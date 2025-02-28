#![no_std]

extern crate alloc;

mod executor;
mod session;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("protocol: {0}")]
    Protocol(#[from] protocol::Error),
    #[error("iwasm: {0}")]
    ContainerError(#[from] wamr_rust_sdk::RuntimeError),
    // Task(&'static str),
    // BufferOverflow,
    // PartialRead,
}

pub trait Transport {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error>;
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error>;
    fn needs_flush(&self) -> bool { false }
}
