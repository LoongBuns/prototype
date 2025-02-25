use std::collections::VecDeque;
use std::future::Future;
use std::io::Result;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use protocol::Message;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct SessionHealth {
    pub retries: u8,
    pub status: SessionStatus,
    pub last_heartbeat: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Connected,
    Occupied,
    Disconnected,
    Zombie,
}

#[derive(Debug, Clone)]
pub struct SessionStream<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    pub inner: Arc<Mutex<T>>,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub device_addr: SocketAddr,
    pub device_ram: u64,
    pub read_buffer: VecDeque<Message>,
    pub write_buffer: VecDeque<Message>,
    pub latency: Duration,
}
