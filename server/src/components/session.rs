use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::BytesMut;
use hecs::Entity;
use protocol::Message;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
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
    pub incoming: BytesMut,
    pub outgoing: BytesMut,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub device_addr: SocketAddr,
    pub device_ram: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub message_queue: VecDeque<Message>,
    pub modules: HashSet<Entity>,
    pub latency: Duration,
}
