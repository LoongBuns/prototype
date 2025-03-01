mod events;
mod transfer;

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use bytes::{Buf, BytesMut};
use events::{EventQueue, SessionEvent};
use log::{error, info, warn};
use protocol::{Message, Type};
use transfer::ModuleTransfer;

use crate::cache::LruCache;
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
    module_cache: LruCache,
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
    const MAX_MODULE_CACHE_SIZE: usize = 2;
    const MAX_BUFF_SIZE: usize = 2048;

    pub fn new(transport: T, executor: E, clock: C, device_ram: u64) -> Self {
        Self {
            transport,
            executor,
            clock,
            shared: RefCell::new(SharedState {
                module_cache: LruCache::new(Self::MAX_MODULE_CACHE_SIZE),
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
        Self::send_ready(&mut self.shared.borrow_mut(), None)?;

        loop {
            self.process_io();
            self.process_events();
        }
    }

    fn process_io(&mut self) {
        let mut shared = self.shared.borrow_mut();

        match self.transport.read(&mut shared.incoming) {
            Ok(n) if n > 0 => {
                while let Ok((message, consumed)) = Message::decode(&shared.incoming) {
                    self.events.borrow_mut().push(SessionEvent::Message(message));
                    shared.incoming.advance(consumed);
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
            let result = self.events.borrow_mut().pop();

            if result.is_some() {
                match result.unwrap() {
                    SessionEvent::Message(message) => self.handle_message(message).unwrap(),
                    SessionEvent::TaskTimeout(task_id) => {
                        if let SessionState::Executing { task_id: id, .. } = self.state {
                            if id == task_id {
                                error!("Task {} timeout", task_id);
                                self.state = SessionState::Failed;
                            }
                        }
                    }
                }

                match &self.state {
                    SessionState::Transferring { task_id, retries, .. } => {
                        let mut shared = self.shared.borrow_mut();

                        if *retries > 3 {
                            error!("Transfer retries exceeded for task {}", task_id);
                            // cannot assign to `self.state` because it is borrowed
                            // `self.state` is assigned to here but it was already borrowedrustcClick for full compiler diagnostic
                            // mod.rs(148, 23): `self.state` is borrowed here
                            // mod.rs(155, 57): borrow later used here
                            // self.state = SessionState::Failed;
                            Self::send_ack(&mut shared, *task_id, None, false).unwrap();
                        }
                    }
                    SessionState::Executing { task_id, deadline } => {
                        if self.clock.timestamp() > *deadline {
                            self.events.borrow_mut().push(SessionEvent::TaskTimeout(*task_id));
                        }
                    }
                    _ => {}
                }
            } else {
                break;
            }
        }
    }

    fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        match msg {
            Message::ServerTask { task_id, module, params } => {
                let module_name = module.name.clone();

                let mut shared = self.shared.borrow_mut();
                if let Some(cached) = shared.module_cache.get(&module_name) {
                    let result = self.executor.execute(cached.to_vec(), params)?;
                    Self::send_result(&mut shared, task_id, result)?;
                } else {
                    let transfer = ModuleTransfer::new(module);
                    self.state = SessionState::Transferring { task_id, transfer, params, retries: 0 };
                    Self::send_ack(&mut shared, task_id, None, true)?;
                }
            }
            Message::ServerModule { task_id, chunk_index, chunk_data } => {
                if let SessionState::Transferring { task_id: idx, transfer, params, retries } = &mut self.state {
                    if *idx != task_id {
                        return Err(Error::InvalidChunk);
                    }

                    if let Err(e) = transfer.add_chunk(chunk_index, &chunk_data) {
                        *retries += 1;
                        return Err(e);
                    }

                    let mut shared = self.shared.borrow_mut();
                    Self::send_ack(&mut shared, task_id, Some(chunk_index), true)?;

                    if transfer.is_complete() {
                        let module_data = transfer.binary()?.to_vec();
                        shared
                            .module_cache
                            .put(transfer.name().to_string(), module_data.to_owned());

                        let result = self.executor.execute(module_data, params.clone())?;
                        Self::send_result(&mut shared, task_id, result)?;
                        self.state = SessionState::Completed;
                    }
                }
            }
            Message::ServerAck { task_id, success } => {
                if let Some(_task) = self.shared.borrow_mut().active_tasks.remove(&task_id) {
                    if success {
                        info!("Task {} completed successfully", task_id);
                    } else {
                        warn!("Task {} failed on server side", task_id);
                    }
                }
            }
            _ => {},
        }
        Ok(())
    }

    #[inline]
    fn send_ready(state: &mut SharedState, name: Option<String>) -> Result<(), Error> {
        let message = Message::ClientReady { module_name: name, device_ram: state.device_ram };
        Self::send_message(state, &message)
    }

    #[inline]
    fn send_ack(state: &mut SharedState, task_id: u64, chunk_index: Option<u32>, success: bool) -> Result<(), Error> {
        let message = Message::ClientAck { task_id, chunk_index, success };
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
