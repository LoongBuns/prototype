use std::error::Error;
use std::sync::Arc;
use std::time::SystemTime;

use hecs::World;
use log::info;
use task::load_modules;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::components::*;
use crate::systems::*;

const CHUNK_SIZE: usize = 1024;

pub async fn run(world: &Arc<Mutex<World>>, addr: &str) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(addr).await?;

    info!("Dispatcher server listening on: {}", listener.local_addr()?);

    let modules = load_modules();
    let tasks = modules.iter().map(|module| {
        (
            Task {
                module_name: module.name.to_owned(),
                module_binary: module.binary.to_owned(),
                params: module.params.to_owned(),
                result: vec![],
                created_at: SystemTime::now(),
                chunk_size: CHUNK_SIZE as u32,
                total_chunks: module.binary.len().div_ceil(CHUNK_SIZE) as u32,
                priority: 1,
            },
            TaskState {
                phase: TaskStatePhase::Queued,
                deadline: None,
                assigned_device: None,
            },
        )
    });
    world.lock().await.spawn_batch(tasks);

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
        LifecycleSystem::maintain_connection(&mut locked).await;
        NetworkSystem::process_inbound::<TcpStream>(&mut locked).await;
        TaskSystem::assign_tasks(&mut locked);
        TaskSystem::distribute_chunks(&mut locked);
        TaskSystem::finalize_tasks(&mut locked);
        NetworkSystem::process_outbound::<TcpStream>(&mut locked).await;
        drop(locked);
    }
}
