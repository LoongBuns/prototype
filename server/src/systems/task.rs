use bitvec::vec::BitVec;
use hecs::{Entity, World};
use protocol::{Message, ModuleMeta};

use crate::components::*;

pub struct TaskSystem;

impl TaskSystem {
    pub fn assign_tasks(world: &mut World) {
        let queued_tasks = world
            .query::<(&Task, &TaskState)>()
            .iter()
            .filter_map(|(entity, (_, state))| match state.phase {
                TaskPhase::Queued => Some(entity),
                _ => None,
            })
            .collect::<Vec<_>>();
        let available_devices = world
            .query::<(&Session, &SessionHealth)>()
            .iter()
            .filter_map(|(entity, (session, health))| match health.status {
                SessionStatus::Connected if session.device_ram > 0 => Some(entity),
                _ => None,
            })
            .collect::<Vec<_>>();

        for (&task_entity, &device_entity) in queued_tasks.iter().zip(available_devices.iter()) {
            let (module, params, total_chunks) = {
                let (task, task_state) = world
                    .query_one_mut::<(&Task, &mut TaskState)>(task_entity)
                    .unwrap();

                task_state.phase = TaskPhase::Distributing;

                let total_chunks = (task.module_binary.len() + task.chunk_size as usize - 1)
                    / task.chunk_size as usize;

                let module = ModuleMeta {
                    name: task.module_name.clone(),
                    size: task.module_binary.len() as u64,
                    chunk_size: task.chunk_size,
                    total_chunks: task.total_chunks,
                };

                (module, task.params.clone(), total_chunks)
            };

            world
                .insert_one(
                    task_entity,
                    TaskTransfer {
                        acked_chunks: BitVec::repeat(false, total_chunks),
                        assigned_device: Some(device_entity),
                        retries: 0,
                    },
                )
                .unwrap();

            if let Ok(session) = world.query_one_mut::<&mut Session>(device_entity) {
                session.write_buffer.push_back(Message::ServerTask {
                    task_id: task_entity.to_bits().into(),
                    module,
                    params,
                });
            }
        }
    }

    pub fn distribute_chunks(world: &mut World) {
        let tasks = world
            .query::<(&Task, &TaskState, &TaskTransfer)>()
            .iter()
            .filter_map(|(entity, (_, state, transfer))| match state.phase {
                TaskPhase::Distributing => Some((entity, transfer.assigned_device)),
                _ => None,
            })
            .collect::<Vec<_>>();

        for (task_entity, device_entity) in tasks {
            if let Some(device_entity) = device_entity {
                let messages = {
                    let (task, transfer) = world
                        .query_one_mut::<(&Task, &TaskTransfer)>(task_entity)
                        .unwrap();
                    Self::generate_chunk_messages(&task_entity, task, &transfer.acked_chunks)
                };

                if let Ok(session) = world.query_one_mut::<&mut Session>(device_entity) {
                    session.write_buffer.extend(messages);
                }

                let (state, transfer) = world
                    .query_one_mut::<(&mut TaskState, &mut TaskTransfer)>(task_entity)
                    .unwrap();
                if transfer.acked_chunks.all() {
                    state.phase = TaskPhase::Executing;
                    world.remove_one::<&TaskTransfer>(task_entity).unwrap();
                }
            }
        }
    }

    fn generate_chunk_messages(
        task_entity: &Entity,
        task: &Task,
        acked_chunks: &BitVec,
    ) -> Vec<Message> {
        let chunks = task.module_binary.chunks(task.chunk_size as usize);
        let mut messages = Vec::new();

        for (chunk_idx, chunk) in chunks.enumerate() {
            if !acked_chunks[chunk_idx] {
                messages.push(Message::ServerModule {
                    task_id: task_entity.to_bits().into(),
                    chunk_index: chunk_idx as u32,
                    chunk_data: chunk.to_vec(),
                });
            }
        }

        messages
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::{Duration, SystemTime};

    use bitvec::prelude::*;
    use hecs::World;
    use protocol::{Message, ModuleMeta, Type};

    use super::*;

    #[test]
    fn test_assign_tasks() {
        let mut world = World::new();
        let task_entity = world.spawn((
            Task {
                module_name: "test".into(),
                module_binary: vec![0u8; 100],
                params: vec![Type::I32(42)],
                result: vec![],
                created_at: SystemTime::now(),
                chunk_size: 50,
                total_chunks: 2,
                priority: 1,
            },
            TaskState {
                phase: TaskPhase::Queued,
                deadline: None,
            },
        ));

        let device_entity = world.spawn((
            Session {
                device_addr: "0.0.0.0:0".parse().unwrap(),
                device_ram: 1,
                read_buffer: VecDeque::new(),
                write_buffer: VecDeque::new(),
                latency: Duration::default(),
            },
            SessionHealth {
                retries: 0,
                status: SessionStatus::Connected,
                last_heartbeat: SystemTime::now(),
            },
        ));

        TaskSystem::assign_tasks(&mut world);

        let task_state = world.get::<&TaskState>(task_entity).unwrap();
        assert_eq!(task_state.phase, TaskPhase::Distributing);

        let session = world.get::<&Session>(device_entity).unwrap();
        assert!(session.write_buffer.iter().any(|msg| matches!(msg, Message::ServerTask { .. })));
    }
}
