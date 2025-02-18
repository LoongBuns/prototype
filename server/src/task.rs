use protocol::Type;

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Queued,
    Dispatched,
    Completed,
    Failed,
}

pub struct Task {
    pub wasm_binary: Vec<u8>,
    pub params: Vec<Type>,
    pub status: TaskStatus,
}

impl Task {
}
