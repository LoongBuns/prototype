#![no_std]

#[macro_use]
extern crate alloc;

mod session;

use alloc::string::String;
use alloc::vec::Vec;

pub use bytes::{Buf, BufMut};
pub use protocol::{Config, Type};
pub use session::Session;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Protocol error: {0}")]
    Protocol(#[from] protocol::Error),
    #[error("Transport error: {0}")]
    Transport(String),
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Invalid chunk")]
    InvalidChunk,
    #[error("Task not found")]
    TaskNotFound,
    #[error("Incomplete write")]
    IncompleteWrite,
    #[error("Cache missing")]
    CacheMiss,
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

    fn read<'a, B>(&mut self, buf: &'a mut B) -> Result<usize, Self::Error>
    where
        B: BufMut + ?Sized;

    fn write<'a, B>(&mut self, src: &'a mut B) -> Result<usize, Self::Error>
    where
        B: Buf;
}
