use std::time::SystemTime;

use bitvec::prelude::BitVec;
use protocol::Type;

use hecs::Entity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskTransferState {
    Prepared,
    Scheduled,
    Retry,
}

#[derive(Debug, Clone)]
pub struct TaskTransfer {
    pub state: TaskTransferState,
    pub acked_chunks: BitVec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatePhase {
    Queued,
    Distributing,
    Executing,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct TaskState {
    pub phase: TaskStatePhase,
    pub deadline: Option<SystemTime>,
    pub assigned_device: Option<Entity>,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub module_name: String,
    pub module_binary: Vec<u8>,
    pub params: Vec<Type>,
    pub result: Vec<Type>,
    pub created_at: SystemTime,
    pub chunk_size: u32,
    pub total_chunks: u32,
    pub priority: u8,
}
