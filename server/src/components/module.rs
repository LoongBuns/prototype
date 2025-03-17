use bitvec::prelude::BitVec;

use hecs::Entity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleTransferState {
    Pending,
    Requested,
    Transferring,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleTransfer {
    pub state: ModuleTransferState,
    pub acked_chunks: BitVec,
    pub session: Entity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub name: String,
    pub binary: Vec<u8>,
    pub dependencies: Vec<Entity>,
    pub chunk_size: u32,
}
