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
                                info!("Session {:?} client ack, chunk id {}", entity, idx);
                            }
                        }
                    }
                    Message::ClientResult { task_id, result } => {
                        if health.status == SessionStatus::Occupied {
                            if let Some(task) = Entity::from_bits(task_id) {
                                task_result.insert(task, result.clone());
                                info!("Task {:?} completed by session {:?}", task, entity);
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
                    debug!("Sent {} bytes to session {:?}", stream.outgoing.len(), entity);
                    stream.outgoing.clear();
                    health.retries = 0;
                }
                Err(e) => {
                    error!("Failed to send {} bytes to session {:?}: {}", stream.outgoing.len(), entity, e);
                    health.retries += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::time::Duration;

    use bitvec::vec::BitVec;
    use bytes::BytesMut;
    use protocol::{Message, ModuleMeta, Type};
    use tokio::io::{AsyncRead, AsyncWrite};
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

    fn create_mock_task(world: &mut World, session_entity: &Entity, size: usize, chunk_size: usize) -> Entity {
        let total_chunks = (size + chunk_size - 1) / chunk_size;
        world.spawn((
            Task {
                module_name: "mock_task".into(),
                module_binary: vec![0u8; size],
                params: vec![Type::I32(0)],
                result: vec![],
                created_at: SystemTime::now(),
                chunk_size: chunk_size as u32,
                total_chunks: total_chunks as u32,
                priority: 1,
            },
            TaskTransfer {
                state: TaskTransferState::Prepared,
                acked_chunks: BitVec::repeat(false, total_chunks),
            },
            TaskState {
                phase: TaskStatePhase::Queued,
                deadline: None,
                assigned_device: Some(session_entity.clone()),
            },
        ))
    }

    #[tokio::test]
    async fn test_process_inbound() {
        let (client, server) = duplex(1024);
        let atomic_client = Arc::new(Mutex::new(client));
        let mut world = World::new();

        let session_entity = create_mock_network(&mut world, &Arc::new(Mutex::new(server)));
        let task_entity = create_mock_task(&mut world, &session_entity, 1024, 256);

        let heartbeat_message = Message::Heartbeat {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64,
        };
        let ready_message = Message::ClientReady {
            device_ram: 2048,
            module_name: None,
        };
        let messages = vec![
            Message::ClientAck {
                task_id: task_entity.to_bits().into(),
                chunk_index: Some(2),
                success: true,
            },
            Message::ClientResult {
                task_id: task_entity.to_bits().into(),
                result: vec![Type::I32(0xcc), Type::I32(0xdd)],
            },
        ];

        assert_eq!(world.get::<&Session>(session_entity).unwrap().latency, Default::default());
        let heartbeat_encoded = heartbeat_message.encode().unwrap();
        atomic_client.lock().await.write_all(&heartbeat_encoded).await.unwrap();
        NetworkSystem::process_inbound::<DuplexStream>(&mut world).await;
        assert!(world.get::<&Session>(session_entity).unwrap().latency.as_nanos() > 0);

        assert_eq!(world.get::<&Session>(session_entity).unwrap().device_ram, 1024);
        let ready_encoded = ready_message.encode().unwrap();
        atomic_client.lock().await.write_all(&ready_encoded).await.unwrap();
        NetworkSystem::process_inbound::<DuplexStream>(&mut world).await;
        assert_eq!(world.get::<&Session>(session_entity).unwrap().device_ram, 2048);

        world.get::<&mut SessionHealth>(session_entity).unwrap().status = SessionStatus::Occupied;

        let mut encoded = messages.iter()
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
        assert_eq!(world.get::<&TaskTransfer>(task_entity).unwrap().acked_chunks[2], true);
        assert_eq!(world.get::<&TaskState>(task_entity).unwrap().phase, TaskStatePhase::Completed);
        assert_eq!(world.get::<&Task>(task_entity).unwrap().result, vec![Type::I32(0xcc), Type::I32(0xdd)]);

        atomic_client.lock().await.shutdown().await.unwrap();
        NetworkSystem::process_inbound::<DuplexStream>(&mut world).await;
        assert_eq!(world.get::<&SessionHealth>(session_entity).unwrap().status, SessionStatus::Disconnected);
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
