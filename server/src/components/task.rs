use std::time::SystemTime;

use bitvec::prelude::BitVec;
use protocol::Type;

use hecs::Entity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskPhase {
    Queued,
    Distributing {
        acked_chunks: BitVec,
        assigned_device: Option<Entity>,
        retries: u8,
    },
    Executing,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct TaskState {
    pub phase: TaskPhase,
    pub deadline: Option<SystemTime>,
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
