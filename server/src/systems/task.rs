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
        #[derive(Debug, Eq, PartialEq)]
        struct TaskRecord {
            entity: Entity,
            module: String,
            size: usize,
        }

        impl Ord for TaskRecord {
            fn cmp(&self, other: &Self) -> Ordering {
                self.size.cmp(&other.size).reverse()
                    .then_with(|| self.entity.cmp(&other.entity).reverse())
                    .then_with(|| self.module.cmp(&other.module))
            }
        }

        impl PartialOrd for TaskRecord {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }

        #[derive(Debug, Eq, PartialEq)]
        struct DeviceRecord {
            entity: Entity,
            cache: HashSet<String>,
            ram: usize,
        }

        impl Ord for DeviceRecord {
            fn cmp(&self, other: &Self) -> Ordering {
                self.ram.cmp(&other.ram)
                    .then_with(|| self.entity.cmp(&other.entity))
            }
        }

        impl PartialOrd for DeviceRecord {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
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
                cache: session.modules.clone(),
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

            let chosen = available_devices[start_idx..]
                .iter()
                .find(|d| d.cache.contains(module_name))
                .or(available_devices[start_idx..].iter().next());

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

                let chunk_count = module.total_chunks as usize;

                let (session, health) = world
                    .query_one_mut::<(&mut Session, &mut SessionHealth)>(device_record.entity)
                    .unwrap();
                health.status = SessionStatus::Occupied;
                session.message_queue.push_back(Message::ServerTask {
                    task_id: task_record.entity.to_bits().into(),
                    module,
                    params,
                });

                world
                    .insert_one(
                        task_record.entity,
                        TaskTransfer {
                            state: TaskTransferState::Pending,
                            acked_chunks: BitVec::repeat(false, chunk_count),
                        },
                    )
                    .unwrap();
            }
        }
    }

    pub fn distribute_chunks(world: &mut World) {
        let distributed_tasks = world
            .query::<(&Task, &TaskState, &TaskTransfer)>()
            .iter()
            .filter_map(|(task_entity, (task, state, transfer))| {
                let device_entity = state.assigned_device?;

                let messages = match transfer.state {
                    TaskTransferState::Requested => task
                        .module_binary
                        .chunks(task.chunk_size as usize)
                        .enumerate()
                        .filter(|(chunk_idx, _)| !transfer.acked_chunks[*chunk_idx])
                        .map(|(chunk_idx, chunk)| Message::ServerModule {
                            task_id: task_entity.to_bits().into(),
                            chunk_index: chunk_idx as u32,
                            chunk_data: chunk.to_vec(),
                        })
                        .collect::<Vec<_>>(),
                    _ => None?,
                };

                Some((task_entity, device_entity, messages))
            })
            .collect::<Vec<_>>();

        for (task_entity, device_entity, messages) in distributed_tasks {
            let mut transfer = world.get::<&mut TaskTransfer>(task_entity).unwrap();
            transfer.state = TaskTransferState::Transferring;

            if let Ok(mut session) = world.get::<&mut Session>(device_entity) {
                debug!("Task {:?} send {} messages to device {:?}", task_entity, messages.len(), device_entity);
                session.message_queue.extend(messages);
            }
        }
    }

    pub fn finalize_tasks(world: &mut World) {
        let finalized_transfer = world
            .query::<(&Task, &TaskState, &TaskTransfer)>()
            .iter()
            .filter_map(|(task_entity, (task, state, transfer))| {
                let device_entity = state.assigned_device?;

                if matches!(transfer.state, TaskTransferState::Transferring) && transfer.acked_chunks.all() {
                    Some((task_entity, device_entity, task.module_name.clone(), state.phase.clone()))
                } else {
                    None?
                }
            })
            .collect::<Vec<_>>();

        for (task_entity, device_entity, module_name, phase) in finalized_transfer {
            if let Ok(mut session) = world.get::<&mut Session>(device_entity) {
                session.modules.insert(module_name);
            }

            if matches!(phase, TaskStatePhase::Distributing) {
                if let Ok(mut state) = world.get::<&mut TaskState>(task_entity) {
                    state.phase = TaskStatePhase::Executing;
                }
            }

            world.remove_one::<TaskTransfer>(task_entity).ok();
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
                modules,
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
        let tasks = (0..6)
            .map(|i| {
                if i % 2 == 0 {
                    create_mock_task(&mut world, "large_task", 50, 16)
                } else {
                    create_mock_task(&mut world, "small_task", 25, 16)
                }
            })
            .collect::<Vec<_>>();
        let large_device = create_mock_device(&mut world, 2048 + 60, &[] as &[&str]);
        let small_device = create_mock_device(&mut world, 2048 + 35, &["small_task"]);

        let test_phases = vec![
            (vec![1, 3], vec![small_device, large_device]),
            (vec![0, 5], vec![large_device, small_device]),
            (vec![2], vec![large_device]),
            (vec![4], vec![large_device]),
        ];

        for (task_indices, expected_devices) in test_phases {
            TaskSystem::assign_tasks(&mut world);

            for (i, &device) in task_indices.iter().zip(expected_devices.iter()) {
                let state = world.get::<&TaskState>(tasks[*i]).unwrap();
                assert_eq!(state.phase, TaskStatePhase::Distributing);
                assert_eq!(state.assigned_device, Some(device));
            }

            for &device in expected_devices.iter() {
                if let Ok(mut health) = world.get::<&mut SessionHealth>(device) {
                    health.status = SessionStatus::Connected;
                }
            }
        }
    }

    #[test]
    fn test_distribute_chunks() {
        let mut world = World::new();
        let task = create_mock_task(&mut world, "mock_task", 25, 16);
        let device = create_mock_device(&mut world, 4096, &[] as &[&str]);

        TaskSystem::assign_tasks(&mut world);
        assert_eq!(world.get::<&Session>(device).unwrap().message_queue.len(), 1);

        world.get::<&mut TaskTransfer>(task).unwrap().state = TaskTransferState::Requested;
        world.get::<&mut Session>(device).unwrap().message_queue.clear();
        TaskSystem::distribute_chunks(&mut world);

        let chunks = world.get::<&Session>(device).unwrap().message_queue
            .iter()
            .map(|message: &Message| match message {
                Message::ServerModule { chunk_data, .. } => chunk_data.len(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(chunks, vec![16, 9]);

        world.get::<&mut TaskTransfer>(task).unwrap().state = TaskTransferState::Requested;
        world.get::<&mut TaskTransfer>(task).unwrap().acked_chunks.set(0, true);
        world.get::<&mut Session>(device).unwrap().message_queue.clear();
        TaskSystem::distribute_chunks(&mut world);
        assert_eq!(world.get::<&Session>(device).unwrap().message_queue.len(), 1);
    }

    #[test]
    fn test_finalize_tasks() {
        let mut world = World::new();
        let task = create_mock_task(&mut world, "mock_task", 25, 16);
        let device = create_mock_device(&mut world, 4096, &[] as &[&str]);

        TaskSystem::assign_tasks(&mut world);
        assert_eq!(world.get::<&Session>(device).unwrap().message_queue.len(), 1);
        world.get::<&mut TaskTransfer>(task).unwrap().state = TaskTransferState::Requested;
        TaskSystem::distribute_chunks(&mut world);

        world.get::<&mut TaskTransfer>(task).unwrap().acked_chunks.set(0, true);
        TaskSystem::finalize_tasks(&mut world);
        assert_eq!(world.get::<&mut TaskTransfer>(task).unwrap().state, TaskTransferState::Transferring);

        world.get::<&mut TaskTransfer>(task).unwrap().acked_chunks.set(1, true);
        TaskSystem::finalize_tasks(&mut world);
        assert!(world.get::<&TaskTransfer>(task).is_err());
    }
}
