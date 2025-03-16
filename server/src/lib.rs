mod components;
mod dispatcher;
mod inspector;
mod systems;

use std::sync::Arc;

use hecs::World;
use tokio::sync::Mutex;

pub use crate::components::*;
pub use crate::systems::*;

pub async fn run(host: &str, ports: &[u16]) {
    let inspector_addr = format!("{}:{}", host, ports[0]);
    let dispatcher_addr = format!("{}:{}", host, ports[1]);

    let world = Arc::new(Mutex::new(World::new()));

    let inspector_world = Arc::clone(&world);
    let inspector_task = tokio::spawn(async move {
        inspector::run(&inspector_world, &inspector_addr).await.unwrap()
    });

    let dispatcher_world = Arc::clone(&world);
    let dispatcher_task = tokio::spawn(async move {
        dispatcher::run(&dispatcher_world, &dispatcher_addr).await.unwrap()
    });

    let (inspector_res, dispatcher_res) = tokio::join!(
        inspector_task,
        dispatcher_task
    );

    inspector_res.unwrap();
    dispatcher_res.unwrap();
}
