mod components;
mod systems;

use std::sync::Arc;
use std::time::SystemTime;

use hecs::World;
use log::info;
use task::load_modules;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

pub use self::components::*;
pub use self::systems::*;

const CHUNK_SIZE: usize = 1024;

pub async fn run(host: &str, port: u16) {
    let addr = format!("{}:{}", host, port);

    let listener = TcpListener::bind(&addr).await.unwrap();
    let world = Arc::new(Mutex::new(World::new()));

    info!("Server listening on {:?}", listener.local_addr());

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
        systems::LifecycleSystem::accept_connection(world_clone, &listener).await;
    });

    loop {
        let mut locked = world.lock().await;
        systems::LifecycleSystem::maintain_connection(&mut locked).await;
        systems::NetworkSystem::process_inbound::<TcpStream>(&mut locked).await;
        systems::TaskSystem::assign_tasks(&mut locked);
        systems::TaskSystem::distribute_chunks(&mut locked);
        systems::TaskSystem::finalize_tasks(&mut locked);
        systems::NetworkSystem::process_outbound::<TcpStream>(&mut locked).await;
        drop(locked);
    }
}
