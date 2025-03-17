use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::time::SystemTime;

use hecs::{Entity, World};
use log::info;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::components::*;
use crate::systems::*;

const CHUNK_SIZE: usize = 1024;

async fn initialize_modules_and_tasks(world: &Arc<Mutex<World>>) {
    let static_modules = task::get_static_modules();
    let mut world_lock = world.lock().await;

    let module_entities = world_lock
        .spawn_batch(static_modules.iter().map(|module| {
            (Module {
                name: module.name.to_string(),
                binary: module.binary.to_vec(),
                dependencies: vec![],
                chunk_size: CHUNK_SIZE as u32,
            },)
        }))
        .collect::<Vec<_>>();

    let module_map = static_modules
        .iter()
        .zip(module_entities.iter())
        .map(|(module, entity)| (module.name.to_string(), *entity))
        .collect::<HashMap<String, Entity>>();

    world
        .lock()
        .await
        .spawn_batch(task::load_tasks().iter().filter_map(|task| {
            Some((
                Task {
                    name: task.name.clone(),
                    params: task.params.to_owned(),
                    result: vec![],
                    created_at: SystemTime::now(),
                    require_module: *module_map.get(&task.module)?,
                    priority: 1,
                },
                TaskState {
                    phase: TaskStatePhase::Queued,
                    assigned_device: None,
                },
            ))
        }));
}

pub async fn run(world: &Arc<Mutex<World>>, addr: &str) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(addr).await?;

    info!("Dispatcher server listening on: {}", listener.local_addr()?);

    initialize_modules_and_tasks(world).await;

    let world_clone = world.clone();
    tokio::spawn(async move {
        while let Ok((stream, addr)) = listener.accept().await {
            info!("Accepted connection from {}", addr);
            let mut world = world_clone.lock().await;
            LifecycleSystem::accept_connection(&mut world, stream, addr);
            drop(world);
        }
    });

    loop {
        let mut locked = world.lock().await;
        LifecycleSystem::maintain_connection(&mut locked, TcpStream::connect).await;
        NetworkSystem::process_inbound::<TcpStream>(&mut locked).await;
        TaskSystem::assign_tasks(&mut locked);
        TaskSystem::transfer_chunks(&mut locked);
        TaskSystem::finalize_transfer(&mut locked);
        NetworkSystem::process_outbound::<TcpStream>(&mut locked).await;
        drop(locked);
    }
}
