use std::time::SystemTime;

use protocol::Type;

use hecs::Entity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatePhase {
    Queued,
    Distributing,
    Executing {
        deadline: SystemTime,
    },
    Completed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskState {
    pub phase: TaskStatePhase,
    pub assigned_device: Option<Entity>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub name: String,
    pub params: Vec<Type>,
    pub result: Vec<Type>,
    pub created_at: SystemTime,
    pub require_module: Entity,
    pub priority: u8,
}
