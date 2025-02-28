use alloc::vec::Vec;

use bitvec::vec::BitVec;
use protocol::{Message, ModuleMeta, Type};

use crate::executor::Executor;
use crate::{Error, Transport};

enum SessionStatus {
    Handshake,
    Idle,
    ReceivingTask {
        task_id: u64,
        meta: ModuleMeta,
        params: Vec<Type>,
        received: BitVec,
        buffer: Vec<u8>,
    },
    ExecutingTask(u64),
}

pub struct Session<T> {
    transport: T,
    executor: Executor,
    state: SessionStatus,
    write_buf: Vec<u8>,
}

impl<T: Transport> Session<T> {
    const MAX_BUF_SIZE: usize = 4096;

    pub fn new(transport: T, executor: Executor) -> Self {
        Self {
            transport,
            executor,
            state: SessionStatus::Handshake,
            write_buf: Vec::with_capacity(Self::MAX_BUF_SIZE),
        }
    }

    fn process_incoming(&mut self) -> Result<(), Error> {
        let mut buf = [0u8; 2048];
        let n = self.transport.read(&mut buf)?;
        let data = &buf[..n];

        let (msg, consumed) = Message::decode(data)?;
        self.handle_message(msg)?;
        
        Ok(())
    }

    fn handle_message(&mut self, msg: Message) -> Result<(), Error> {
        match msg {
            Message::ServerTask { task_id, module, params } => {
                let task = TaskPool::alloc().ok_or(Error::TaskFull)?;
                let params = Vec::from_slice(&params).map_err(|_| Error::BufferFull)?;
                
                *task = TaskState {
                    task_id,
                    meta: module,
                    params,
                    received_data: BytesMut::with_capacity(module.size as usize),
                    received_chunks: BitVec::repeat(false, module.total_chunks as usize),
                };
                self.tasks.push(task).map_err(|_| Error::TaskFull)?;
            }
            
            Message::ServerModule { task_id, chunk_index, chunk_data } => {
                if let Some(task) = self.tasks.iter_mut().find(|t| t.task_id == task_id) {
                    let start = chunk_index as usize * task.meta.chunk_size as usize;
                    let end = start + chunk_data.len();
                    
                    if task.received_data.len() < end {
                        task.received_data.resize(end, 0);
                    }
                    task.received_data[start..end].copy_from_slice(&chunk_data);
                    task.received_chunks.set(chunk_index as usize, true);
                    
                    self.queue_ack(task_id, chunk_index)?;
                }
            }
            
            Message::Heartbeat { timestamp } => {
                self.last_heartbeat = Some(timestamp);
                self.queue_heartbeat(timestamp)?;
            }
            
            _ => return Err(Error::Protocol(ProtocolError::InvalidMessage)),
        }
        Ok(())
    }
}
