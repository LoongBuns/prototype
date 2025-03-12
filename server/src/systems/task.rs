use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

use bitvec::vec::BitVec;
use hecs::{Entity, World};
use log::{debug, info};
use protocol::{Message, ModuleMeta};

use crate::components::*;

pub struct TaskSystem;

impl TaskSystem {
    pub fn assign_tasks(world: &mut World) {
        #[derive(Eq, PartialEq)]
        struct TaskRecord {
            entity: Entity,
            module: String,
            size: usize,
        }

        impl Ord for TaskRecord {
            fn cmp(&self, other: &Self) -> Ordering {
                self.size.cmp(&other.size).reverse()
            }
        }

        impl PartialOrd for TaskRecord {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                self.size.partial_cmp(&other.size).map(Ordering::reverse)
            }
        }

        #[derive(Eq, PartialEq)]
        struct DeviceRecord {
            entity: Entity,
            cache: HashSet<String>,
            ram: usize,
        }

        impl Ord for DeviceRecord {
            fn cmp(&self, other: &Self) -> Ordering {
                self.ram.cmp(&other.ram)
            }
        }

        impl PartialOrd for DeviceRecord {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                self.ram.partial_cmp(&other.ram)
            }
        }

        let mut queued_tasks = world
            .query::<(&Task, &TaskState)>()
            .without::<&TaskTransfer>()
            .iter()
            .filter(|&(_, (_, state))| state.phase == TaskStatePhase::Queued)
            .map(|(entity, (task, _))| TaskRecord {
                entity,
                module: task.module_name.clone(),
                size: task.module_binary.len() + 2048,
            })
            .collect::<BinaryHeap<_>>();

        let available_devices = world
            .query::<(&Session, &SessionHealth)>()
            .iter()
            .filter(|&(_, (_, health))| health.status == SessionStatus::Connected)
            .map(|(entity, (session, _))| DeviceRecord {
                ram: session.device_ram as usize,
                cache: session.cached_modules.clone(),
                entity,
            })
            .collect::<BinaryHeap<_>>();

        let mut available_devices = available_devices
            .into_sorted_vec()
            .into_iter()
            .collect::<Vec<_>>();

        while let Some(task_record) = queued_tasks.pop() {
            let required_ram = task_record.size;
            let module_name = &task_record.module;

            let start_idx = available_devices.partition_point(|d| d.ram < required_ram);

            let chosen = available_devices[start_idx..].iter().fold(
                (None, None),
                |(mut cached, mut uncached), d| {
                    if d.cache.contains(module_name) {
                        if cached.map_or(true, |c: &DeviceRecord| d.ram < c.ram) {
                            cached = Some(d);
                        }
                    } else if uncached.map_or(true, |c: &DeviceRecord| d.ram < c.ram) {
                        uncached = Some(d);
                    }
                    (cached, uncached)
                },
            );
            let chosen = chosen.0.or(chosen.1);

            if let Some(device) = chosen {
                let pos = available_devices
                    .iter()
                    .position(|d| d.entity == device.entity)
                    .unwrap();
                let device_record = available_devices.swap_remove(pos);

                let (module, params) = {
                    let (task, state) = world
                        .query_one_mut::<(&Task, &mut TaskState)>(task_record.entity)
                        .unwrap();
                    state.phase = TaskStatePhase::Distributing;
                    state.assigned_device = Some(device_record.entity);
                    info!("Task {:?} assigned to device {:?}", task_record.entity, device_record.entity);
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

                if let Ok((session, health)) = world.query_one_mut::<(&mut Session, &mut SessionHealth)>(device_record.entity) {
                    let chunk_count = module.total_chunks as usize;
                    let module_name = module.name.to_owned();
                    health.status = SessionStatus::Occupied;

                    session.message_queue.push_back(Message::ServerTask {
                        task_id: task_record.entity.to_bits().into(),
                        module,
                        params,
                    });

                    if session.cached_modules.contains(&module_name) {
                        if let Ok(mut state) = world.get::<&mut TaskState>(task_record.entity) {
                            state.phase = TaskStatePhase::Executing;
                            info!("Task {:?} found shortcut device {:?}, moving to executing phase", task_record.entity, device_record.entity);
                        }
                    } else {
                        world
                            .insert_one(
                                task_record.entity,
                                TaskTransfer {
                                    state: TaskTransferState::Prepared,
                                    acked_chunks: BitVec::repeat(false, chunk_count),
                                },
                            )
                            .unwrap();
                    }
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
            let (module_name, finish) = {
                let (task, transfer) = world
                    .query_one_mut::<(&Task, &TaskTransfer)>(task_entity)
                    .unwrap();
                (task.module_name.clone(), transfer.acked_chunks.all())
            };

            if finish {
                if let Ok(mut session) = world.get::<&mut Session>(device_entity) {
                    session.cached_modules.insert(module_name);
                }

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
    use std::collections::{HashSet, VecDeque};
    use std::time::{Duration, SystemTime};

    use hecs::Entity;
    use protocol::Type;

    use super::*;

    fn create_mock_task(world: &mut World, module_name: &str, size: usize, chunk_size: usize) -> Entity {
        world.spawn((
            Task {
                module_name: module_name.into(),
                module_binary: vec![0u8; size],
                params: vec![Type::I32(0)],
                result: vec![],
                created_at: SystemTime::now(),
                chunk_size: chunk_size as u32,
                total_chunks: (size.div_ceil(chunk_size)) as u32,
                priority: 1,
            },
            TaskState {
                phase: TaskStatePhase::Queued,
                deadline: None,
                assigned_device: None,
            },
        ))
    }

    fn create_mock_device(world: &mut World, ram: usize, cached: &[impl AsRef<str>]) -> Entity {
        let modules = cached
            .iter()
            .map(|s| s.as_ref().to_string())
            .collect::<HashSet<_>>();

        world.spawn((
            Session {
                device_addr: "0.0.0.0:0".parse().unwrap(),
                device_ram: ram as u64,
                message_queue: VecDeque::new(),
                latency: Duration::default(),
                cached_modules: modules,
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
        let large_task = create_mock_task(&mut world, "large_task", 50, 16);
        let small_task = create_mock_task(&mut world, "small_task", 25, 16);
        let large_device = create_mock_device(&mut world, 2048 + 60, &[] as &[&str]);
        let small_device = create_mock_device(&mut world, 2048 + 35, &["small_task"]); 

        TaskSystem::assign_tasks(&mut world);

        let large_state = world.get::<&TaskState>(large_task).unwrap();
        let small_state = world.get::<&TaskState>(small_task).unwrap();
        assert_eq!(large_state.phase, TaskStatePhase::Distributing);
        assert_eq!(large_state.assigned_device, Some(large_device));
        assert_eq!(small_state.phase, TaskStatePhase::Executing);
        assert_eq!(small_state.assigned_device, Some(small_device));

        let session = world.get::<&Session>(large_device).unwrap();
        let message = session.message_queue.front().unwrap();
        assert!(matches!(message, Message::ServerTask { task_id, .. } if *task_id == u64::from(large_task.to_bits())));
        let session = world.get::<&Session>(small_device).unwrap();
        let message = session.message_queue.front().unwrap();
        assert!(matches!(message, Message::ServerTask { task_id, .. } if *task_id == u64::from(small_task.to_bits())));
    }

    #[test]
    fn test_distribute_chunks() {
        let mut world = World::new();
        let task = create_mock_task(&mut world, "mock_task", 30, 16);
        let device = create_mock_device(&mut world, 4096, &[] as &[&str]);

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
