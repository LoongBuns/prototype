mod components;
mod module;
mod systems;

use std::time::SystemTime;

use components::*;
use hecs::World;
use log::info;
use module::load_modules;
use tokio::net::{TcpListener, TcpStream};

const CHUNK_SIZE: usize = 1024;

pub async fn run(host: &str, port: u16) {
    let addr = format!("{}:{}", host, port);

    let listener = TcpListener::bind(&addr).await.unwrap();
    let mut world = World::new();

    info!("Server listening on {:?}", listener.local_addr());

    let modules = load_modules();
    let tasks = modules
        .iter()
        .map(|module| (
            Task {
                module_name: module.name.to_owned(),
                module_binary: module.binary.to_owned(),
                params: module.params.to_owned(),
                result: vec![],
                created_at: SystemTime::now(),
                chunk_size: CHUNK_SIZE as u32,
                total_chunks: ((module.binary.len() + CHUNK_SIZE - 1) / CHUNK_SIZE) as u32,
                priority: 1,
            },
            TaskState {
                phase: TaskPhase::Queued,
                deadline: None,
            },
        ));
    world.spawn_batch(tasks);

    loop {
        systems::LifecycleSystem::accept_connection(&mut world, &listener).await;
        systems::LifecycleSystem::maintain_connection(&mut world).await;
        systems::NetworkSystem::process_inbound::<TcpStream>(&mut world).await;
        systems::TaskSystem::assign_tasks(&mut world);
        systems::TaskSystem::distribute_chunks(&mut world);
        systems::NetworkSystem::process_outbound::<TcpStream>(&mut world).await;
    }
}
