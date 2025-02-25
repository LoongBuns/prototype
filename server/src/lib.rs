mod components;
mod module;
mod systems;

use std::time::Duration;

use hecs::World;
use tokio::net::{TcpListener, TcpStream};

pub async fn run(host: &str, port: u16) {
    let addr = format!("{}:{}", host, port);
    println!("Server listening on {}", addr);

    let listener = TcpListener::bind(&addr).await.unwrap();
    let mut world = World::new();

    loop {
        systems::LifecycleSystem::accept(&mut world, &listener).await;
        systems::LifecycleSystem::maintain_health(&mut world);
        systems::NetworkSystem::process_inbound::<TcpStream>(&mut world).await;
        systems::NetworkSystem::process_message(&mut world).await;
        systems::TaskSystem::assign_tasks(&mut world);
        systems::TaskSystem::distribute_chunks(&mut world);
        systems::NetworkSystem::process_outbound::<TcpStream>(&mut world).await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
