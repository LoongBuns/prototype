请创建一个新client crate用于运行接收并执行代码，我的要求是这个crate是个lib项目，其中尽量使用泛型抽象实现，对于任务执行部分可以空白，我会补完。

* 外部protocol项目（请不要修改）

```rust
#![no_std]

extern crate alloc;

mod config;

use alloc::string::String;
use alloc::vec::Vec;

pub use config::{Config, Wifi};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Insufficient data")]
    InsufficientData,
    #[error("Invalid message")]
    InvalidMessage,
    #[error("Decode error: {0:?}")]
    DecodeError(bincode::error::DecodeError),
    #[error("Encode error: {0:?}")]
    EncodeError(bincode::error::EncodeError),
}

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, PartialEq)]
pub enum Type {
    Void,
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128(i128),
}

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, PartialEq)]
pub struct ModuleMeta {
    pub name: String,
    pub size: u64,
    pub chunk_size: u32,
    pub total_chunks: u32,
}

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, PartialEq)]
pub enum Message {
    ClientReady {
        module_name: Option<String>,
        device_ram: u64,
    },
    ServerTask {
        task_id: u64,
        module: ModuleMeta,
        params: Vec<Type>,
    },
    ServerModule {
        task_id: u64,
        chunk_index: u32,
        chunk_data: Vec<u8>,
    },
    ClientAck {
        task_id: u64,
        chunk_index: Option<u32>,
        success: bool,
    },
    ClientResult {
        task_id: u64,
        result: Vec<Type>,
    },
    ServerAck {
        task_id: u64,
        success: bool,
    },
    Heartbeat {
        timestamp: u64,
    },
}

impl Message {
    const HEADER_SIZE: usize = 2;

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let config = bincode::config::standard()
            .with_variable_int_encoding()
            .with_big_endian();
        let payload = bincode::encode_to_vec(self, config).map_err(Error::EncodeError)?;
        let payload_len = payload.len();

        if payload_len > u16::MAX as usize {
            return Err(Error::InvalidMessage);
        }

        let mut output = Vec::with_capacity(Self::HEADER_SIZE + payload_len);
        output.extend_from_slice(&(payload_len as u16).to_be_bytes());
        output.extend(payload);

        Ok(output)
    }

    pub fn decode(data: &[u8]) -> Result<(Self, usize), Error> {
        if data.len() < Self::HEADER_SIZE {
            return Err(Error::InsufficientData);
        }

        let payload_len = u16::from_be_bytes([data[0], data[1]]) as usize;
        let total_len = Self::HEADER_SIZE + payload_len;

        if data.len() < total_len {
            return Err(Error::InsufficientData);
        }

        let config = bincode::config::standard()
            .with_variable_int_encoding()
            .with_big_endian();

        let (message, size) =
            bincode::decode_from_slice(&data[Self::HEADER_SIZE..total_len], config)
                .map_err(Error::DecodeError)?;

        if size != payload_len {
            return Err(Error::InvalidMessage);
        }

        Ok((message, total_len))
    }
}
```

* components/task.rs

```rust
use std::time::SystemTime;

use bitvec::prelude::BitVec;
use protocol::Type;

use hecs::Entity;

#[derive(Debug, Clone)]
pub struct TaskTransfer {
    pub acked_chunks: BitVec,
    pub assigned_device: Option<Entity>,
    pub retries: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskPhase {
    Queued,
    Distributing,
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
```

* components/session.rs

```rust
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::BytesMut;
use protocol::Message;
use tokio::io::{AsyncRead, AsyncWrite};
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
    pub incoming: BytesMut,
    pub outgoing: BytesMut,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub device_addr: SocketAddr,
    pub device_ram: u64,
    pub message_queue: VecDeque<Message>,
    pub latency: Duration,
}
```

* systems/lifecycle.rs

```rust
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::BytesMut;
use hecs::World;
use log::{info, warn};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::components::*;

pub struct LifecycleSystem;

impl LifecycleSystem {
    const MAX_RETRIES: u8 = 5;
    const TIMEOUT: Duration = Duration::from_secs(30);

    pub async fn accept_connection(world: &mut World, listener: &TcpListener) {
        if let Ok((stream, addr)) = listener.accept().await {
            info!("Accepted connection from {}", addr);
            world.spawn((
                Session {
                    device_addr: addr,
                    device_ram: 0,
                    message_queue: VecDeque::new(),
                    latency: Duration::default(),
                },
                SessionStream {
                    inner: Arc::new(Mutex::new(stream)),
                    incoming: BytesMut::new(),
                    outgoing: BytesMut::new(),
                },
                SessionHealth {
                    retries: 0,
                    status: SessionStatus::Connected,
                    last_heartbeat: SystemTime::now(),
                },
            ));
        }
    }

    pub async fn maintain_connection(world: &mut World) {
        let mut pending_reconnects = Vec::new();
        let mut dead_sessions = Vec::new();
        let now = SystemTime::now();

        for (entity, (session, health)) in world.query::<(&mut Session, &mut SessionHealth)>().iter() {
            let elapsed = now
                .duration_since(health.last_heartbeat)
                .unwrap_or_default();

            match health.status {
                SessionStatus::Connected if elapsed > Self::TIMEOUT => {
                    warn!("Session {:?} timed out ({} secs), marked as zombie", entity, elapsed.as_secs());
                    health.status = SessionStatus::Zombie;
                    health.retries = 0;
                }
                SessionStatus::Zombie => {
                    health.retries += 1;
                    if health.retries >= Self::MAX_RETRIES {
                        info!("Session {:?} reached max retries, scheduled for removal", entity);
                        dead_sessions.push(entity);
                    }
                }
                SessionStatus::Disconnected => {
                    info!("Session {:?} disconnected, attempting reconnect", entity);
                    pending_reconnects.push((entity, session.device_addr));
                }
                _ => {}
            }
        }

        for (entity, addr) in pending_reconnects {
            if let Ok(stream) = TcpStream::connect(addr).await {
                info!("Session {:?} reconnected to {} successfully", entity, addr);
                if let Ok((session, health)) = world
                    .query_one_mut::<(&mut SessionStream<TcpStream>, &mut SessionHealth)>(entity)
                {
                    session.inner = Arc::new(Mutex::new(stream));
                    health.status = SessionStatus::Connected;
                    health.last_heartbeat = SystemTime::now();
                }
            }
        }

        for entity in dead_sessions {
            info!("Removing dead session {:?}", entity);
            world.despawn(entity).ok();
        }
    }
}
```

* systems/network.rs

```rust
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Buf;
use hecs::{Entity, World};
use log::{debug, error, info};
use protocol::Message;
use tokio::io::*;

use crate::components::*;

pub struct NetworkSystem;

impl NetworkSystem {
    pub async fn process_inbound<T>(world: &mut World)
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let mut task_transfer = HashMap::new();
        let mut task_result = HashMap::new();

        for (entity, (session, stream, health)) in world
            .query::<(&mut Session, &mut SessionStream<T>, &mut SessionHealth)>()
            .iter()
        {
            let mut locked_stream = match stream.inner.try_lock() {
                Ok(stream) => stream,
                Err(_) => continue,
            };

            match locked_stream.read_buf(&mut stream.incoming).await {
                Ok(0) => {
                    info!("Session {:?} closed connection gracefully", entity);
                    health.status = SessionStatus::Disconnected;
                    continue;
                }
                Err(e) => {
                    error!("Session {:?} read buf error: {}", entity, e);
                    health.status = SessionStatus::Disconnected;
                    continue;
                }
                _ => {}
            }

            while let Ok((message, consumed)) = Message::decode(&stream.incoming) {
                stream.incoming.advance(consumed);
                let now = SystemTime::now();

                match message {
                    Message::Heartbeat { timestamp } => {
                        let last_record = UNIX_EPOCH + Duration::from_nanos(timestamp);
                        let latency = now.duration_since(last_record).unwrap();
                        session.latency = latency;
                        debug!("Session {:?} heartbeat, latency {} ms", entity, latency.as_millis());
                    }
                    Message::ClientReady { device_ram, .. } => {
                        if health.status == SessionStatus::Connected {
                            session.device_ram = device_ram;
                            info!("Session {:?} client ready, device ram {}", entity, device_ram);
                        }
                    }
                    Message::ClientAck { task_id, chunk_index, success } => {
                        if health.status == SessionStatus::Occupied {
                            if let (Some(task), Some(idx)) = (Entity::from_bits(task_id), chunk_index) {
                                task_transfer.entry(task).or_insert(Vec::new()).push((idx, success));
                            }
                        }
                    }
                    Message::ClientResult { task_id, result } => {
                        if health.status == SessionStatus::Occupied {
                            if let Some(task) = Entity::from_bits(task_id) {
                                info!("Task {:?} completed by session {:?}", task, entity);
                                task_result.insert(task, result.clone());
                            }
                        }
                    }
                    _ => {}
                };

                health.last_heartbeat = now;
            }
        }

        for (entity, chunks) in task_transfer {
            if let Ok(transfer) = world.query_one_mut::<&mut TaskTransfer>(entity) {
                for (idx, success) in chunks {
                    transfer.acked_chunks.set(idx as usize, success);
                }
            }
        }

        for (entity, result) in task_result {
            if let Ok((task, state)) = world.query_one_mut::<(&mut Task, &mut TaskState)>(entity) {
                task.result = result.to_owned();
                state.phase = TaskPhase::Completed;
            }
        }
    }

    pub async fn process_outbound<T>(world: &mut World)
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        for (entity, (session, stream, health)) in world
            .query::<(&mut Session, &mut SessionStream<T>, &mut SessionHealth)>()
            .iter()
        {
            let mut locked_stream = match stream.inner.try_lock() {
                Ok(stream) => stream,
                Err(_) => continue,
            };

            while let Some(msg) = session.message_queue.pop_front() {
                if let Ok(data) = msg.encode() {
                    stream.outgoing.extend(data);
                }
            }

            if let Err(e) = locked_stream.write_all(&stream.outgoing).await {
                error!("Failed to send data to session {:?}: {}", entity, e);
                health.retries += 1;
            } else {
                debug!("Sent {} bytes to session {:?}", stream.outgoing.len(), entity);
                stream.outgoing.clear();
                health.retries = 0;
            }
        }
    }
}
```

* systems/task.rs

```rust
use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bitvec::vec::BitVec;
use hecs::World;
use log::{debug, info};
use protocol::{Message, ModuleMeta};

use crate::components::*;

pub struct TaskSystem;

impl TaskSystem {
    pub fn assign_tasks(world: &mut World) {
        let mut queued_tasks = world
            .query::<(&Task, &TaskState)>()
            .iter()
            .filter_map(|(entity, (task, state))| {
                (state.phase == TaskPhase::Queued)
                    .then(|| (Reverse(task.module_binary.len() + 2048), entity))
            })
            .collect::<BinaryHeap<_>>();

        let available_devices = world
            .query::<(&Session, &SessionHealth)>()
            .iter()
            .filter_map(|(entity, (session, health))| {
                (health.status == SessionStatus::Connected)
                    .then(|| (Reverse(session.device_ram as usize), entity))
            })
            .collect::<BinaryHeap<_>>();

        let mut available_devices = available_devices
            .into_sorted_vec()
            .into_iter()
            .map(|(r, e)| (r.0, e))
            .collect::<Vec<_>>();

        while let Some((Reverse(task_cost), task_entity)) = queued_tasks.pop() {
            if let Some(pos) = available_devices
                .iter()
                .position(|&(ram, _)| ram >= task_cost)
            {
                let (_, device_entity) = available_devices.swap_remove(pos);

                let (module, params) = {
                    let (task, state) = world
                        .query_one_mut::<(&Task, &mut TaskState)>(task_entity)
                        .unwrap();
                    state.phase = TaskPhase::Distributing;
                    info!("Task {:?} assigned to device {:?}", task_entity, device_entity);
                    (
                        ModuleMeta {
                            name: task.module_name.clone(),
                            size: task.module_binary.len() as u64,
                            chunk_size: task.chunk_size,
                            total_chunks: task.total_chunks,
                        },
                        task.params.clone(),
                    )
                };

                world
                    .insert_one(
                        task_entity,
                        TaskTransfer {
                            acked_chunks: BitVec::repeat(false, module.total_chunks as usize),
                            assigned_device: Some(device_entity),
                            retries: 0,
                        },
                    )
                    .unwrap();

                if let Ok(session) = world.query_one_mut::<&mut Session>(device_entity) {
                    session.message_queue.push_back(Message::ServerTask {
                        task_id: task_entity.to_bits().into(),
                        module,
                        params,
                    });
                }
            }
        }
    }

    pub fn distribute_chunks(world: &mut World) {
        let distributing_tasks = world
            .query::<(&Task, &TaskState, &TaskTransfer)>()
            .iter()
            .filter_map(|(task_entity, (task, _, transfer))| {
                transfer.assigned_device.map(|device_entity| {
                    let messages = task
                        .module_binary
                        .chunks(task.chunk_size as usize)
                        .enumerate()
                        .filter_map(|(chunk_idx, chunk)| {
                            (!transfer.acked_chunks[chunk_idx]).then(|| Message::ServerModule {
                                task_id: task_entity.to_bits().into(),
                                chunk_index: chunk_idx as u32,
                                chunk_data: chunk.to_vec(),
                            })
                        })
                        .collect::<Vec<_>>();

                    (task_entity, device_entity, messages)
                })
            })
            .collect::<Vec<_>>();

        for (task_entity, device_entity, messages) in distributing_tasks {
            if let Ok(mut session) = world.get::<&mut Session>(device_entity) {
                session.message_queue.extend(messages);
                debug!("Task {:?} send {} chunks to device {:?}", task_entity, session.message_queue.len(), device_entity);
            }

            let finish = world.get::<&TaskTransfer>(task_entity).unwrap().acked_chunks.all();
            if finish {
                if let Ok(mut state) = world.get::<&mut TaskState>(task_entity) {
                    state.phase = TaskPhase::Executing;
                    info!("Task {:?} all chunks acknowledged, moving to executing phase", task_entity);
                }

                world.remove_one::<TaskTransfer>(task_entity).ok();
            }
        }
    }
}
```

```rust
mod components;
mod module;
mod systems;

use hecs::World;
use log::info;
use tokio::net::{TcpListener, TcpStream};

pub async fn run(host: &str, port: u16) {
    let addr = format!("{}:{}", host, port);
    info!("Server listening on {}", addr);

    let listener = TcpListener::bind(&addr).await.unwrap();
    let mut world = World::new();

    loop {
        systems::LifecycleSystem::accept_connection(&mut world, &listener).await;
        systems::LifecycleSystem::maintain_connection(&mut world).await;
        systems::NetworkSystem::process_inbound::<TcpStream>(&mut world).await;
        systems::TaskSystem::assign_tasks(&mut world);
        systems::TaskSystem::distribute_chunks(&mut world);
        systems::NetworkSystem::process_outbound::<TcpStream>(&mut world).await;
    }
}
```