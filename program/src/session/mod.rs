mod cache;
mod events;
mod transfer;

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use bytes::{Buf, BytesMut};
use cache::ModuleCache;
use events::{EventQueue, SessionEvent};
use log::{error, info, warn};
use protocol::{AckInfo, Message, Type};
use transfer::ModuleTransfer;

use crate::{Clock, Error, Executor, Transport};

pub struct TaskMeta {
    pub module: String,
    pub params: Vec<Type>,
}

impl TaskMeta {
    pub fn new(name: String, params: Vec<Type>) -> Self {
        Self {
            module: name,
            params,
        }
    }
}

pub enum SessionState {
    Ready,
    Transferring {
        task_id: u64,
        transfer: ModuleTransfer,
        params: Vec<Type>,
        retries: u8,
    },
    Executing {
        task_id: u64,
        deadline: u64,
    },
    Completed,
    Failed,
}

struct SharedState {
    module_cache: ModuleCache,
    active_tasks: BTreeMap<u64, TaskMeta>,
    incoming: BytesMut,
    outgoing: BytesMut,
    device_ram: u64,
}

pub struct Session<T: Transport, E: Executor, C: Clock> {
    transport: T,
    executor: E,
    clock: C,
    shared: RefCell<SharedState>,
    state: SessionState,
    events: RefCell<EventQueue>,
}

impl<T: Transport, E: Executor, C: Clock> Session<T, E, C> {
    const MAX_MODULE_CACHE_SIZE: usize = 1024 * 64;
    const MAX_BUFF_SIZE: usize = 2048;

    pub fn new(transport: T, executor: E, clock: C, device_ram: u64) -> Self {
        Self {
            transport,
            executor,
            clock,
            shared: RefCell::new(SharedState {
                module_cache: ModuleCache::new(Self::MAX_MODULE_CACHE_SIZE),
                active_tasks: BTreeMap::new(),
                incoming: BytesMut::with_capacity(Self::MAX_BUFF_SIZE),
                outgoing: BytesMut::with_capacity(Self::MAX_BUFF_SIZE),
                device_ram,
            }),
            state: SessionState::Ready,
            events: RefCell::new(EventQueue::new()),
        }
    }

    pub fn run(&mut self) -> Result<(), Error> {
        Self::send_ready(&mut self.shared.borrow_mut(), Vec::new())?;

        loop {
            self.process_io();
            self.process_events();
            self.process_state();
        }
    }

    fn process_io(&mut self) {
        let mut shared = self.shared.borrow_mut();

        match self.transport.read(&mut shared.incoming) {
            Ok(n) if n > 0 => {
                loop {
                    match Message::decode(&shared.incoming) {
                        Ok((message, consumed)) => {
                            self.events.borrow_mut().push(SessionEvent::Message(message));
                            shared.incoming.advance(consumed);
                        }
                        Err(_) => break,
                    }
                }
            }
            Err(e) => {
                error!("Transport read error: {:?}", e);
                self.state = SessionState::Failed;
            }
            _ => {}
        }

        while !shared.outgoing.is_empty() {
            let write_result = self.transport.write(&mut shared.outgoing);
            match write_result {
                Ok(n) => {
                    shared.outgoing.advance(n);
                    if n == 0 {
                        warn!("Zero bytes written, connection may be closed");
                        break;
                    }
                }
                Err(e) => {
                    error!("Transport write error: {:?}", e);
                    self.state = SessionState::Failed;
                }
            }
        }
    }

    fn process_events(&mut self) {
        loop {
            let event = self.events.borrow_mut().pop();
            if let Some(event) = event.as_ref() {
                match event {
                    SessionEvent::Message(msg) => {
                        if let Err(e) = self.handle_message(&msg) {
                            error!("Resolve message error: {:?}", e);
                            self.state = SessionState::Failed;
                            break;
                        }
                    }
                    SessionEvent::TaskTimeout(task_id) => {
                        warn!("Task {} timed out", task_id);
                        if let SessionState::Executing { task_id: current_id, .. } = self.state {
                            if current_id == *task_id {
                                self.state = SessionState::Failed;
                                break;
                            }
                        }
                    }
                }
            } else {
                break;
            }
        }
    }

    fn process_state(&mut self) {
        match &mut self.state {
            SessionState::Transferring { task_id, retries, .. } => {
                let mut shared = self.shared.borrow_mut();
                if *retries > 3 {
                    let modules: Vec<String> = shared.module_cache.keys();
                    Self::send_ack(&mut shared, *task_id, AckInfo::Task { modules }).unwrap();
                    self.state = SessionState::Failed;
                }
            }
            SessionState::Executing { task_id, deadline } => {
                if self.clock.timestamp() > *deadline {
                    self.events
                        .borrow_mut()
                        .push(SessionEvent::TaskTimeout(*task_id));
                }
            }
            _ => {}
        }
    }

    fn handle_message(&mut self, msg: &Message) -> Result<(), Error> {
        match msg {
            Message::ServerTask { task_id, module, params } => {
                info!("Received ServerTask id {} module {} params {:?}", task_id, module.name, params);
                let module_name = module.name.clone();
                let mut shared = self.shared.borrow_mut();

                let modules: Vec<String> = shared.module_cache.keys();
                Self::send_ack(&mut shared, *task_id, AckInfo::Task { modules })?;

                if let Some(cached) = shared.module_cache.get(&module_name) {
                    let result = self
                        .executor
                        .execute(cached, params.to_owned())
                        .map_err(|e| Error::Execution(e.to_string()))?;
                    Self::send_result(&mut shared, *task_id, result)?;
                } else {
                    shared
                        .module_cache
                        .put(&module_name, module.size as usize)?;

                    if shared.module_cache.contains_key(&module_name) {
                        let transfer = ModuleTransfer::new(module);
                        self.state = SessionState::Transferring {
                            task_id: *task_id,
                            transfer,
                            params: params.to_owned(),
                            retries: 0,
                        };
                    } else {
                        self.state = SessionState::Failed;
                    }
                }
            }
            Message::ServerModule { task_id, chunk_index, chunk_data } => {
                if let SessionState::Transferring {
                    task_id: current_id,
                    transfer,
                    params,
                    retries,
                } = &mut self.state
                {
                    if *current_id != *task_id {
                        return Err(Error::TaskNotFound(*task_id));
                    }

                    let mut shared = self.shared.borrow_mut();
                    match transfer.add_chunk(
                        &mut shared.module_cache,
                        *chunk_index as usize,
                        chunk_data,
                    ) {
                        Ok(_) => {
                            Self::send_ack(&mut shared, *task_id, AckInfo::Module {
                                chunk_index: *chunk_index,
                                success: true,
                            })?;

                            if transfer.is_complete() {
                                info!("Module transfer completed for task {:?}", task_id);
                                let module_name = transfer.name().to_string();
                                let module_data = shared
                                    .module_cache
                                    .get(&module_name)
                                    .ok_or(Error::CacheEntryNotFound(module_name))?;

                                let result = self
                                    .executor
                                    .execute(module_data, params.clone())
                                    .map_err(|e| Error::Execution(e.to_string()))?;
                                Self::send_result(&mut shared, *task_id, result)?;
                                self.state = SessionState::Completed;
                            }
                        }
                        Err(e) => {
                            Self::send_ack(&mut shared, *task_id, AckInfo::Module {
                                chunk_index: *chunk_index,
                                success: false,
                            })?;
                            *retries += 1;
                            return Err(e);
                        }
                    }
                }
            }
            Message::ServerAck { task_id, success } => {
                if let Some(_task) = self.shared.borrow_mut().active_tasks.remove(task_id) {
                    if *success {
                        info!("Task {} completed successfully", task_id);
                    } else {
                        warn!("Task {} failed on server side", task_id);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    #[inline]
    fn send_ready(state: &mut SharedState, modules: Vec<String>) -> Result<(), Error> {
        let message = Message::ClientReady { modules, device_ram: state.device_ram };
        Self::send_message(state, &message)
    }

    #[inline]
    fn send_ack(state: &mut SharedState, task_id: u64, ack_info: AckInfo) -> Result<(), Error> {
        let message = Message::ClientAck { task_id, ack_info };
        Self::send_message(state, &message)
    }

    #[inline]
    fn send_result(state: &mut SharedState, task_id: u64, result: Vec<Type>) -> Result<(), Error> {
        let message = Message::ClientResult { task_id, result };
        Self::send_message(state, &message)
    }

    #[inline]
    fn send_heartbeat(state: &mut SharedState, timestamp: u64) -> Result<(), Error> {
        let message = Message::Heartbeat { timestamp };
        Self::send_message(state, &message)
    }

    #[inline]
    fn send_message(state: &mut SharedState, message: &Message) -> Result<(), Error> {
        let data = message.encode()?;
        state.outgoing.extend_from_slice(&data);
        Ok(())
    }
}
