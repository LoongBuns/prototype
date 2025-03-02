mod components;
mod module;
mod systems;

use hecs::World;
use log::info;
use tokio::net::{TcpListener, TcpStream};

pub async fn run(host: &str, port: u16) {
    let addr = format!("{}:{}", host, port);

    let listener = TcpListener::bind(&addr).await.unwrap();
    let mut world = World::new();

    info!("Server listening on {:?}", listener.local_addr());

    loop {
        systems::LifecycleSystem::accept_connection(&mut world, &listener).await;
        systems::LifecycleSystem::maintain_connection(&mut world).await;
        systems::NetworkSystem::process_inbound::<TcpStream>(&mut world).await;
        systems::TaskSystem::assign_tasks(&mut world);
        systems::TaskSystem::distribute_chunks(&mut world);
        systems::NetworkSystem::process_outbound::<TcpStream>(&mut world).await;
    }
}
