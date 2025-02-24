use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use protocol::Message;
use tokio::net::TcpStream;
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
    Working,
    Disconnected,
    Zombie,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub device_addr: SocketAddr,
    pub device_ram: u64,
    pub stream: Arc<Mutex<TcpStream>>,
    pub read_buffer: VecDeque<Message>,
    pub write_buffer: VecDeque<Message>,
    pub latency: Duration,
}
