use std::collections::HashMap;
use std::time::SystemTime;

use bytes::{Buf, BytesMut};
use hecs::{Entity, World};
use protocol::Message;
use tokio::io::*;

use crate::components::*;

pub struct NetworkSystem;

impl NetworkSystem {
    pub async fn process_inbound<T>(world: &mut World)
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        for (_, (session, stream, health)) in world
            .query::<(&mut Session, &mut SessionStream<T>, &mut SessionHealth)>()
            .iter()
        {
            let mut buffer = BytesMut::new();
            loop {
                match Message::decode(&buffer) {
                    Ok((message, consumed)) => {
                        buffer.advance(consumed);
                        session.read_buffer.push_back(message);
                        health.last_heartbeat = SystemTime::now();
                        break;
                    }
                    Err(protocol::Error::InsufficientData) => {
                        let mut locked_stream = stream.inner.lock().await;
                        match locked_stream.read_buf(&mut buffer).await {
                            Ok(0) => health.status = SessionStatus::Disconnected,
                            Ok(_) => {}
                            Err(_) => health.retries += 1,
                        }
                    }
                    Err(_) => {}
                }
            }
        }
    }

    pub async fn process_outbound<T>(world: &mut World)
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        for (_, (session, stream, health)) in world
            .query::<(&mut Session, &mut SessionStream<T>, &mut SessionHealth)>()
            .iter()
        {
            if session.write_buffer.is_empty() {
                continue;
            }

            while let Some(message) = session.write_buffer.pop_front() {
                match message.encode() {
                    Ok(buffer) => {
                        let mut locked_stream = stream.inner.lock().await;
                        if let Err(_) = locked_stream.write_all(&buffer).await {
                            health.retries += 1;
                        }
                    }
                    Err(_) => {}
                }
            }
        }
    }

    pub async fn process_message(world: &mut World) {
        let mut task_transfer_process = HashMap::new();
        let mut task_result = HashMap::new();

        for (_, (session, health)) in world.query::<(&mut Session, &mut SessionHealth)>().iter() {
            match health.status {
                SessionStatus::Connected => {
                    session.read_buffer.retain(|message| match message {
                        Message::ClientReady { device_ram, .. } => {
                            session.device_ram = *device_ram;
                            false
                        }
                        _ => true,
                    });
                }
                SessionStatus::Occupied => {
                    session.read_buffer.retain(|message| match message {
                        Message::ClientAck {
                            task_id,
                            chunk_index,
                            success,
                        } => {
                            if let Some(task) = Entity::from_bits(*task_id) {
                                let process =
                                    task_transfer_process.entry(task).or_insert(Vec::new());
                                if let Some(idx) = chunk_index {
                                    process.push((*idx, *success));
                                }
                            }
                            false
                        }
                        Message::ClientResult { task_id, result } => {
                            if let Some(task) = Entity::from_bits(*task_id) {
                                task_result.insert(task, result.clone());
                            }
                            false
                        }
                        _ => true,
                    });
                }
                _ => {}
            }
        }

        for (entity, buffer) in task_transfer_process.iter() {
            let transfer = world.query_one_mut::<&mut TaskTransfer>(*entity).unwrap();
            for (chunk_index, success) in buffer {
                transfer.acked_chunks.set(*chunk_index as usize, *success);
            }
        }

        for (entity, buffer) in task_result.iter() {
            let (task, state) = world
                .query_one_mut::<(&mut Task, &mut TaskState)>(*entity)
                .unwrap();
            task.result = buffer.to_owned();
            state.phase = TaskPhase::Completed;
        }
    }
}
