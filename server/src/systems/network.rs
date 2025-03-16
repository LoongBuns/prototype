use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Buf;
use hecs::{Entity, World};
use log::{debug, error, info};
use protocol::{AckInfo, Message};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

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
                    error!("Session {:?} read stream failed: {}", entity, e);
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
                        info!(
                            "Session {entity:?} received heartbeat with latency {}ms",
                            latency.as_millis()
                        );
                        session.latency = latency;
                    }
                    Message::ClientReady { modules, device_ram } => {
                        if health.status == SessionStatus::Connected {
                            info!(
                                "Session {:?} received client ready with cached module {:?} and ram {}",
                                entity, modules, device_ram
                            );
                            session.modules.clear();
                            session.modules.extend(modules);
                            session.device_ram = device_ram;
                        }
                    }
                    Message::ClientAck { task_id, ack_info } => {
                        if health.status == SessionStatus::Occupied {
                            if let Some(task) = Entity::from_bits(task_id) {
                                info!(
                                    "Session {:?} received client ack with info {:?} for task {:?}",
                                    entity, ack_info, task
                                );
                                if let AckInfo::Task { modules } = &ack_info {
                                    session.modules.clear();
                                    session.modules.extend(modules.clone());
                                }
                                task_transfer
                                    .entry(task)
                                    .or_insert(Vec::new())
                                    .push(ack_info);
                            }
                        }
                    }
                    Message::ClientResult { task_id, result } => {
                        if health.status == SessionStatus::Occupied {
                            if let Some(task) = Entity::from_bits(task_id) {
                                info!(
                                    "Session {:?} received client result with result {:?} for task {:?}",
                                    entity, result, task
                                );
                                task_result.insert(task, result.clone());
                            }

                            health.status = SessionStatus::Connected
                        }
                    }
                    _ => {}
                };

                health.last_heartbeat = now;
            }
        }

        for (entity, acks) in task_transfer {
            if let Ok((task, transfer)) = world.query_one_mut::<(&Task, &mut TaskTransfer)>(entity) {
                for ack_info in acks {
                    match ack_info {
                        AckInfo::Module { chunk_index, success } => {
                            transfer.acked_chunks.set(chunk_index as usize, success);
                        }
                        AckInfo::Task { modules } => {
                            transfer.state = TaskTransferState::Requested;
                            if modules.contains(&task.module_name) {
                                transfer.acked_chunks.fill(true);
                                break;
                            }
                        }
                    }
                }
            }
        }

        for (entity, result) in task_result {
            let mut device_entity = None;
            if let Ok((task, state)) = world.query_one_mut::<(&mut Task, &mut TaskState)>(entity) {
                device_entity = state.assigned_device;
                task.result = result.to_owned();
                state.phase = TaskStatePhase::Completed;
            }
            if let Some(device_entity) = device_entity {
                if let Ok(mut session) = world.get::<&mut Session>(device_entity) {
                    session.message_queue.push_back(Message::ServerAck {
                        task_id: entity.to_bits().into(),
                        success: true,
                    });
                }
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

            if stream.outgoing.is_empty() {
                continue;
            }

            match locked_stream.write_all(&stream.outgoing).await {
                Ok(_) => {
                    debug!(
                        "Sent {} bytes to session {:?}",
                        stream.outgoing.len(),
                        entity
                    );
                    stream.outgoing.clear();
                    health.retries = 0;
                }
                Err(e) => {
                    error!(
                        "Failed to send {} bytes to session {:?}: {}",
                        stream.outgoing.len(),
                        entity,
                        e
                    );
                    health.retries += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashSet, VecDeque};
    use std::sync::Arc;

    use bitvec::prelude::*;
    use bytes::BytesMut;
    use protocol::{ModuleMeta, Type};
    use tokio::io::{duplex, DuplexStream};
    use tokio::sync::Mutex;

    use super::*;

    fn create_mock_network<T>(world: &mut World, stream: &Arc<Mutex<T>>) -> Entity
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        world.spawn((
            Session {
                device_addr: "0.0.0.0:0".parse().unwrap(),
                device_ram: 1024,
                message_queue: VecDeque::new(),
                latency: Duration::default(),
                modules: HashSet::new(),
            },
            SessionStream {
                inner: stream.clone(),
                incoming: BytesMut::new(),
                outgoing: BytesMut::new(),
            },
            SessionHealth {
                retries: 0,
                status: SessionStatus::Connected,
                last_heartbeat: SystemTime::now(),
            },
        ))
    }

    fn create_mock_task(world: &mut World, session_entity: &Entity) -> Entity {
        const TOTAL_SIZE: usize = 1024;
        const CHUNK_SIZE: usize = 256;

        let total_chunks = TOTAL_SIZE.div_ceil(CHUNK_SIZE);
        world.spawn((
            Task {
                module_name: "mock_task".into(),
                module_binary: vec![0u8; TOTAL_SIZE],
                params: vec![Type::I32(0)],
                result: vec![],
                created_at: SystemTime::now(),
                chunk_size: CHUNK_SIZE as u32,
                total_chunks: total_chunks as u32,
                priority: 1,
            },
            TaskTransfer {
                state: TaskTransferState::Requested,
                acked_chunks: bitvec![0; total_chunks],
            },
            TaskState {
                phase: TaskStatePhase::Queued,
                deadline: None,
                assigned_device: Some(*session_entity),
            },
        ))
    }

    #[tokio::test]
    async fn test_process_inbound_heartbeat() {
        let (mut client, server) = duplex(1024);
        let mut world = World::new();

        let session_entity = create_mock_network(&mut world, &Arc::new(Mutex::new(server)));

        let message = Message::Heartbeat {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        };

        let latency = world.get::<&Session>(session_entity).unwrap().latency;
        assert_eq!(latency, Default::default());
        let encoded = message.encode().unwrap();
        client.write_all(&encoded).await.unwrap();
        NetworkSystem::process_inbound::<DuplexStream>(&mut world).await;
        let latency = world.get::<&Session>(session_entity).unwrap().latency;
        assert!(latency.as_nanos() > 0);
    }

    #[tokio::test]
    async fn test_process_inbound_ready() {
        let (mut client, server) = duplex(1024);
        let mut world = World::new();

        let session_entity = create_mock_network(&mut world, &Arc::new(Mutex::new(server)));

        let message = Message::ClientReady {
            modules: Vec::new(),
            device_ram: 2048,
        };

        let ram = world.get::<&Session>(session_entity).unwrap().device_ram;
        assert_eq!(ram, 1024);
        let encoded = message.encode().unwrap();
        client.write_all(&encoded).await.unwrap();
        NetworkSystem::process_inbound::<DuplexStream>(&mut world).await;
        let ram = world.get::<&Session>(session_entity).unwrap().device_ram;
        assert_eq!(ram, 2048);
    }

    #[tokio::test]
    async fn test_process_inbound_ack_result() {
        let (client, server) = duplex(1024);
        let atomic_client = Arc::new(Mutex::new(client));
        let mut world = World::new();

        let session_entity = create_mock_network(&mut world, &Arc::new(Mutex::new(server)));
        let task_entity = create_mock_task(&mut world, &session_entity);

        let messages = [
            Message::ClientAck {
                task_id: task_entity.to_bits().into(),
                ack_info: AckInfo::Module {
                    chunk_index: 2,
                    success: true,
                },
            },
            Message::ClientResult {
                task_id: task_entity.to_bits().into(),
                result: vec![Type::I32(0xcc), Type::I32(0xdd)],
            },
        ];

        world
            .get::<&mut SessionHealth>(session_entity)
            .unwrap()
            .status = SessionStatus::Occupied;

        let mut encoded = messages
            .iter()
            .map(|m| m.encode().unwrap())
            .collect::<Vec<_>>()
            .concat();
        let half = encoded.split_off(encoded.len() / 2);
        let client_owned = atomic_client.to_owned();
        let job_handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            client_owned.lock().await.write_all(&half).await.unwrap();
            client_owned.lock().await.flush().await.unwrap();
        });

        atomic_client.lock().await.write_all(&encoded).await.unwrap();
        NetworkSystem::process_inbound::<DuplexStream>(&mut world).await;
        job_handle.await.unwrap();
        NetworkSystem::process_inbound::<DuplexStream>(&mut world).await;
        let acked = &world.get::<&TaskTransfer>(task_entity).unwrap().acked_chunks;
        assert_eq!(*acked, bits![0, 0, 1, 0]);
        let phase = &world.get::<&TaskState>(task_entity).unwrap().phase;
        assert_eq!(*phase, TaskStatePhase::Completed);
        let result = &world.get::<&Task>(task_entity).unwrap().result;
        assert_eq!(*result, vec![Type::I32(0xcc), Type::I32(0xdd)]);
    }

    #[tokio::test]
    async fn test_process_inbound_disconnect() {
        let (mut client, server) = duplex(1024);
        let mut world = World::new();

        let session_entity = create_mock_network(&mut world, &Arc::new(Mutex::new(server)));

        client.shutdown().await.unwrap();
        NetworkSystem::process_inbound::<DuplexStream>(&mut world).await;
        let status = &world.get::<&SessionHealth>(session_entity).unwrap().status;
        assert_eq!(*status, SessionStatus::Disconnected);
    }

    #[tokio::test]
    async fn test_process_outbound() {
        let (mut client, server) = duplex(1024);
        let mut world = World::new();
        let session_entity = create_mock_network(&mut world, &Arc::new(Mutex::new(server)));

        if let Ok(mut session) = world.get::<&mut Session>(session_entity) {
            session.message_queue.push_back(Message::ServerTask {
                task_id: 0,
                module: ModuleMeta {
                    name: "mock_task".into(),
                    size: 1024,
                    chunk_size: 256,
                    total_chunks: 4,
                },
                params: vec![Type::I32(0xaa), Type::I32(0xbb)],
            });
        };

        NetworkSystem::process_outbound::<DuplexStream>(&mut world).await;

        let mut buf = BytesMut::new();
        client.read_buf(&mut buf).await.unwrap();
        let decoded = Message::decode(&buf[..]).unwrap().0;
        assert!(matches!(decoded, Message::ServerTask { .. }));
    }
}
