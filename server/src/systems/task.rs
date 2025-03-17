use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::time::{Duration, SystemTime};

use bitvec::vec::BitVec;
use hecs::{Entity, World};
use log::{debug, info};
use protocol::{Message, ModuleInfo};

use crate::components::*;

pub struct TaskSystem;

impl TaskSystem {
    pub fn assign_tasks(world: &mut World) {
        #[derive(Debug, Eq, PartialEq)]
        struct TaskRecord {
            entity: Entity,
            module_entity: Entity,
            size: usize,
            chunk_size: usize,
            priority: u8,
        }

        impl Ord for TaskRecord {
            fn cmp(&self, other: &Self) -> Ordering {
                self.priority.cmp(&other.priority).reverse()
                    .then_with(|| self.size.cmp(&other.size).reverse())
                    .then_with(|| self.module_entity.cmp(&other.module_entity).reverse())
                    .then_with(|| self.entity.cmp(&other.entity).reverse())
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
            module_entities: HashSet<Entity>,
            ram: usize,
        }

        let mut queued_tasks = world
            .query::<(&Task, &TaskState)>()
            .iter()
            .filter(|&(_, (_, state))| matches!(state.phase, TaskStatePhase::Queued))
            .filter_map(|(entity, (task, _))| {
                let module = world.get::<&Module>(task.require_module).ok()?;
                Some(TaskRecord {
                    entity,
                    module_entity: task.require_module,
                    size: module.binary.len(),
                    chunk_size: module.chunk_size as usize,
                    priority: task.priority,
                })
            })
            .collect::<BinaryHeap<_>>();

        let mut device_map = world
            .query::<(&Session, &SessionHealth, &SessionInfo)>()
            .iter()
            .filter(|&(_, (_, health, _))| matches!(health.status, SessionStatus::Connected))
            .map(|(entity, (session, _, info))| {
                (entity, DeviceRecord {
                    entity,
                    module_entities: session.modules.clone(),
                    ram: info.device_ram as usize,
                })
            })
            .collect::<HashMap<_, _>>();

        while let Some(task_record) = queued_tasks.pop() {
            let required_ram = task_record.size + 2048;

            let target_device = {
                let mut suitable_devices = device_map.values_mut()
                    .filter(|d| d.ram >= required_ram)
                    .collect::<Vec<_>>();

                let best_device_with_cache = suitable_devices.iter_mut()
                    .filter(|d| d.module_entities.contains(&task_record.module_entity))
                    .max_by_key(|d| Reverse(d.ram));

                if let Some(device) = best_device_with_cache {
                    Some(device.entity)
                } else {
                    suitable_devices.iter_mut()
                        .max_by_key(|d| d.ram)
                        .map(|d| d.entity)
                }
            }.and_then(|e| device_map.remove(&e));

            if let Some(device) = target_device {
                let total_chunks = task_record.size.div_ceil(task_record.chunk_size) as u32;

                let params = world
                    .get::<&Task>(task_record.entity)
                    .unwrap()
                    .params
                    .clone();

                let module = {
                    let task = world
                        .get::<&Task>(task_record.entity)
                        .unwrap();
                    let mut state = world
                        .get::<&mut TaskState>(task_record.entity)
                        .unwrap();

                    let module = world
                        .get::<&Module>(task.require_module)
                        .unwrap();

                    state.phase = TaskStatePhase::Distributing;
                    state.assigned_device = Some(device.entity);
                    info!("Task {:?} assigned to device {:?}", task_record.entity, device.entity);
                    ModuleInfo {
                        name: module.name.clone(),
                        size: module.binary.len() as u64,
                        chunk_size: task_record.chunk_size as u32,
                        total_chunks,
                    }
                };

                let chunk_count = module.total_chunks as usize;

                let (session, health) = world
                    .query_one_mut::<(&mut Session, &mut SessionHealth)>(device.entity)
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
                        ModuleTransfer {
                            state: ModuleTransferState::Pending,
                            acked_chunks: BitVec::repeat(false, chunk_count),
                            session: device.entity,
                        },
                    )
                    .unwrap();
            }
        }
    }

    pub fn transfer_chunks(world: &mut World) {
        let module_transfers = world
            .query::<(&Task, &ModuleTransfer)>()
            .iter()
            .filter_map(|(task_entity, (task, transfer))| {
                let module = world.get::<&Module>(task.require_module).ok()?;
                let device_entity = transfer.session;

                let messages = match transfer.state {
                    ModuleTransferState::Requested => module
                        .binary
                        .chunks(module.chunk_size as usize)
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

        for (task_entity, device_entity, messages) in module_transfers {
            let mut transfer = world.get::<&mut ModuleTransfer>(task_entity).unwrap();
            transfer.state = ModuleTransferState::Transferring;

            if let Ok(mut session) = world.get::<&mut Session>(device_entity) {
                debug!("Task {:?} send {} messages to device {:?}", task_entity, messages.len(), device_entity);
                session.message_queue.extend(messages);
            }
        }
    }

    pub fn finalize_transfer(world: &mut World) {
        let completed_transfers = world
            .query::<(&TaskState, &ModuleTransfer)>()
            .iter()
            .filter_map(|(entity, (state, transfer))| {
                if transfer.acked_chunks.all() {
                    state.assigned_device.map(|device| (entity, device))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        for (module_entity, session_entity) in completed_transfers {
            if let Ok(mut session) = world.get::<&mut Session>(session_entity) {
                session.modules.insert(module_entity);
            }

            for (_, (_, state)) in world
                .query::<(&Task, &mut TaskState)>()
                .iter()
                .filter(|(_, (task, _))| task.require_module == module_entity)
            {
                state.phase = TaskStatePhase::Executing {
                    deadline: SystemTime::now() + Duration::from_secs(60),
                }
            }

            world.remove_one::<ModuleTransfer>(module_entity).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::{Duration, SystemTime};

    use hecs::Entity;
    use protocol::Type;

    use super::*;

    fn create_mock_module(world: &mut World, name: &str, size: usize, chunk_size: usize) -> Entity {
        world.spawn((
            Module {
                name: name.to_string(),
                binary: vec![0u8; size],
                dependencies: vec![],
                chunk_size: chunk_size as u32,
            },
        ))
    }

    fn create_mock_task(world: &mut World, name: &str, module_entity: &Entity, priority: u8) -> Entity {
        world.spawn((
            Task {
                name: name.to_string(),
                params: vec![Type::I32(0)],
                result: vec![],
                created_at: SystemTime::now(),
                require_module: *module_entity,
                priority,
            },
            TaskState {
                phase: TaskStatePhase::Queued,
                assigned_device: None,
            },
        ))
    }

    fn create_mock_device(world: &mut World, ram: usize, cached: &[Entity]) -> Entity {
        world.spawn((
            Session {
                message_queue: VecDeque::new(),
                modules: cached.iter().cloned().collect(),
                latency: Duration::default(),
            },
            SessionInfo {
                device_addr: "0.0.0.0:0".parse().unwrap(),
                device_ram: ram as u64,
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
        let large_module = create_mock_module(&mut world, "large_module", 50, 16);
        let small_module = create_mock_module(&mut world, "small_module", 25, 16);
        let tasks = (0..6)
            .map(|i| {
                if i % 2 == 0 {
                    create_mock_task(&mut world, "large_task", &large_module, 1)
                } else {
                    create_mock_task(&mut world, "small_task", &small_module, 1)
                }
            })
            .collect::<Vec<_>>();
        let large_device = create_mock_device(&mut world, 2048 + 60, &[]);
        let small_device = create_mock_device(&mut world, 2048 + 35, &[small_module]);

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
                log::info!("{:?}", state);
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
    fn test_transfer_chunks() {
        let mut world = World::new();
        let module = create_mock_module(&mut world, "mock_module", 25, 16);
        let task = create_mock_task(&mut world, "mock_task", &module, 1);
        let device = create_mock_device(&mut world, 4096, &[]);

        TaskSystem::assign_tasks(&mut world);
        assert_eq!(world.get::<&Session>(device).unwrap().message_queue.len(), 1);

        world.get::<&mut ModuleTransfer>(task).unwrap().state = ModuleTransferState::Requested;
        world.get::<&mut Session>(device).unwrap().message_queue.clear();
        TaskSystem::transfer_chunks(&mut world);

        let chunks = world.get::<&Session>(device).unwrap().message_queue
            .iter()
            .map(|message: &Message| match message {
                Message::ServerModule { chunk_data, .. } => chunk_data.len(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert_eq!(chunks, vec![16, 9]);

        world.get::<&mut ModuleTransfer>(task).unwrap().state = ModuleTransferState::Requested;
        world.get::<&mut ModuleTransfer>(task).unwrap().acked_chunks.set(0, true);
        world.get::<&mut Session>(device).unwrap().message_queue.clear();
        TaskSystem::transfer_chunks(&mut world);
        assert_eq!(world.get::<&Session>(device).unwrap().message_queue.len(), 1);
    }

    #[test]
    fn test_finalize_tasks() {
        let mut world = World::new();
        let module = create_mock_module(&mut world, "mock_module", 25, 16);
        let task = create_mock_task(&mut world, "mock_task", &module, 1);
        let device = create_mock_device(&mut world, 4096, &[]);

        TaskSystem::assign_tasks(&mut world);
        assert_eq!(world.get::<&Session>(device).unwrap().message_queue.len(), 1);
        world.get::<&mut ModuleTransfer>(task).unwrap().state = ModuleTransferState::Requested;
        TaskSystem::transfer_chunks(&mut world);

        world.get::<&mut ModuleTransfer>(task).unwrap().acked_chunks.set(0, true);
        TaskSystem::finalize_transfer(&mut world);
        assert_eq!(world.get::<&mut ModuleTransfer>(task).unwrap().state, ModuleTransferState::Transferring);

        world.get::<&mut ModuleTransfer>(task).unwrap().acked_chunks.set(1, true);
        TaskSystem::finalize_transfer(&mut world);
        assert!(world.get::<&ModuleTransfer>(task).is_err());
    }
}
