#![no_std]

#[macro_use]
extern crate alloc;

mod session;

use alloc::string::String;
use alloc::vec::Vec;

pub use bytes::{Buf, BufMut};
pub use protocol::{Config, Type};
pub use session::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Protocol error: {0}")]
    Protocol(#[from] protocol::Error),
    #[error("Transport error: {0}")]
    Transport(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Invalid chunk index ({0} in range [0, {1}])")]
    InvalidChunkIndex(usize, usize),
    #[error("Duplicate chunk (index: {0})")]
    DuplicateChunk(usize),
    #[error("Invalid chunk size (expected {0}, got {1})")]
    InvalidChunkSize(usize, usize),
    #[error("Task not found: {0}")]
    TaskNotFound(u64),
    #[error("Cache entry not found: {0}")]
    CacheEntryNotFound(String),
    #[error("Cache full (allocated: {0}/{1})")]
    CacheFull(usize, usize),
}

pub trait Clock {
    fn timestamp(&self) -> u64;
}

pub trait Executor {
    type Error: core::error::Error;

    fn execute(&self, module: &[u8], params: Vec<Type>) -> Result<Vec<Type>, Self::Error>;
}

pub trait Transport {
    type Error: core::error::Error;

    fn read<B>(&mut self, buf: &mut B) -> Result<usize, Self::Error>
    where
        B: BufMut + ?Sized;

    fn write<B>(&mut self, src: &mut B) -> Result<usize, Self::Error>
    where
        B: Buf;
}
