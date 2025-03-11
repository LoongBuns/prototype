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
            .without::<&TaskTransfer>()
            .iter()
            .filter(|&(_, (_, state))| state.phase == TaskStatePhase::Queued)
            .map(|(entity, (task, _))| (Reverse(task.module_binary.len() + 2048), entity))
            .collect::<BinaryHeap<_>>();

        let available_devices = world
            .query::<(&Session, &SessionHealth)>()
            .iter()
            .filter(|&(_, (_, health))| health.status == SessionStatus::Connected)
            .map(|(entity, (session, _))| (Reverse(session.device_ram as usize), entity))
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
                    state.phase = TaskStatePhase::Distributing;
                    state.assigned_device = Some(device_entity);
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
                            state: TaskTransferState::Prepared,
                            acked_chunks: BitVec::repeat(false, module.total_chunks as usize),
                        },
                    )
                    .unwrap();

                if let Ok((session, health)) = world.query_one_mut::<(&mut Session, &mut SessionHealth)>(device_entity) {
                    health.status = SessionStatus::Occupied;
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
            .filter_map(|(task_entity, (task, state, transfer))| {
                state.assigned_device.map(|device_entity| {
                    let messages = task
                        .module_binary
                        .chunks(task.chunk_size as usize)
                        .enumerate()
                        .filter(|(chunk_idx, _)| {
                            !matches!(transfer.state, TaskTransferState::Scheduled)
                                && !transfer.acked_chunks[*chunk_idx]
                        })
                        .map(|(chunk_idx, chunk)| Message::ServerModule {
                            task_id: task_entity.to_bits().into(),
                            chunk_index: chunk_idx as u32,
                            chunk_data: chunk.to_vec(),
                        })
                        .collect::<Vec<_>>();

                    (task_entity, device_entity, messages)
                })
            })
            .collect::<Vec<_>>();

        for (task_entity, device_entity, messages) in distributing_tasks {
            let finish = world.get::<&TaskTransfer>(task_entity).unwrap().acked_chunks.all();
            if finish {
                if let Ok(mut state) = world.get::<&mut TaskState>(task_entity) {
                    state.phase = TaskStatePhase::Executing;
                    info!("Task {:?} all chunks acknowledged, moving to executing phase", task_entity);
                }

                world.remove_one::<TaskTransfer>(task_entity).ok();
            } else if !messages.is_empty() {
                let mut transfer = world.get::<&mut TaskTransfer>(task_entity).unwrap();
                transfer.state = TaskTransferState::Scheduled;

                if let Ok(mut session) = world.get::<&mut Session>(device_entity) {
                    session.message_queue.extend(messages);
                    debug!("Task {:?} send {} messages to device {:?}", task_entity, session.message_queue.len(), device_entity);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::{Duration, SystemTime};

    use hecs::{Entity, World};
    use protocol::{Message, Type};

    use super::*;

    fn create_mock_task(world: &mut World, size: usize, chunk_size: usize) -> Entity {
        world.spawn((
            Task {
                module_name: "mock_task".into(),
                module_binary: vec![0u8; size],
                params: vec![Type::I32(0)],
                result: vec![],
                created_at: SystemTime::now(),
                chunk_size: chunk_size as u32,
                total_chunks: ((size + chunk_size - 1) / chunk_size) as u32,
                priority: 1,
            },
            TaskState {
                phase: TaskStatePhase::Queued,
                deadline: None,
                assigned_device: None,
            },
        ))
    }

    fn create_mock_device(world: &mut World, ram: usize) -> Entity {
        world.spawn((
            Session {
                device_addr: "0.0.0.0:0".parse().unwrap(),
                device_ram: ram as u64,
                message_queue: VecDeque::new(),
                latency: Duration::default(),
            },
            SessionHealth {
                retries: 0,
                status: SessionStatus::Connected,
                last_heartbeat: SystemTime::now(),
            },
        ))
    }

    #[test]
    fn test_assign_tasks() {
        let mut world = World::new();
        let task = create_mock_task(&mut world, 30, 16);
        let device = create_mock_device(&mut world, 4096);

        TaskSystem::assign_tasks(&mut world);

        let task_state = world.get::<&TaskState>(task).unwrap();
        assert_eq!(task_state.phase, TaskStatePhase::Distributing);

        let session = world.get::<&Session>(device).unwrap();
        let message = session.message_queue.front().unwrap();
        assert!(matches!(message, Message::ServerTask { .. }));
    }

    #[test]
    fn test_distribute_chunks() {
        let mut world = World::new();
        let task = create_mock_task(&mut world, 30, 16);
        let device = create_mock_device(&mut world, 4096);

        TaskSystem::assign_tasks(&mut world);
        TaskSystem::distribute_chunks(&mut world);

        let chunks = world.get::<&Session>(device).unwrap().message_queue
            .iter()
            .map(|message: &Message| match message {
                Message::ServerModule { chunk_data, .. } => chunk_data.len(),
                Message::ServerTask { .. } => usize::MIN,
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(chunks, vec![usize::MIN, 16, 14]);

        world.get::<&mut TaskTransfer>(task).unwrap().state = TaskTransferState::Retry;
        world.get::<&mut TaskTransfer>(task).unwrap().acked_chunks.set(0, true);
        TaskSystem::distribute_chunks(&mut world);
        assert_eq!(world.get::<&Session>(device).unwrap().message_queue.len(), 4);

        world.get::<&mut TaskTransfer>(task).unwrap().acked_chunks.set(1, true);
        TaskSystem::distribute_chunks(&mut world);
        assert!(world.get::<&TaskTransfer>(task).is_err());
        assert_eq!(world.get::<&TaskState>(task).unwrap().phase, TaskStatePhase::Executing);
    }
}
