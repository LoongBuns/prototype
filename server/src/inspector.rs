use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use hecs::{ChangeTracker, World};
use log::info;
use tokio::net::TcpListener;
use tokio::sync::{watch, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::components::*;

struct InspectorState {
    world: Arc<Mutex<World>>,
    version: Arc<watch::Sender<usize>>,
    task_tracker: Arc<Mutex<ChangeTracker<Task>>>,
    task_state_tracker: Arc<Mutex<ChangeTracker<TaskState>>>,
}

unsafe impl Send for InspectorState {}

impl InspectorState {
    pub fn new(world: Arc<Mutex<hecs::World>>) -> Self {
        let (version_tx, _) = watch::channel(0);

        Self {
            world,
            version: Arc::new(version_tx),
            task_tracker: Arc::new(Mutex::new(ChangeTracker::new())),
            task_state_tracker: Arc::new(Mutex::new(ChangeTracker::new())),
        }
    }

    pub async fn trigger_updates(&mut self) {
        let mut world = self.world.lock().await;

        let task_changes = {
            let mut locked = self.task_tracker.lock().await;
            let mut task_tracker = locked.track(&mut world);
            task_tracker.added().len() > 0
                || task_tracker.changed().count() > 0
                || task_tracker.removed().len() > 0
        };

        let task_state_changes = {
            let mut locked = self.task_state_tracker.lock().await;
            let mut task_state_tracker = locked.track(&mut world);
            task_state_tracker.added().len() > 0
                || task_state_tracker.changed().count() > 0
                || task_state_tracker.removed().len() > 0
        };

        if task_changes || task_state_changes {
            self.version.send_modify(|v| *v += 1);
        }
    }
}

pub async fn run(world: &Arc<Mutex<World>>, addr: &str) -> Result<(), Box<dyn Error>> {
    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
    let static_files_service = ServeDir::new(assets_dir)
        .append_index_html_on_directories(true);

    let listener = TcpListener::bind(addr).await?;
    info!("Inspector server listening on: {}", listener.local_addr()?);

    let state = InspectorState::new(world.clone());

    let app = Router::new()
        .fallback_service(static_files_service)
        // .with_state(state)
        .layer(CorsLayer::permissive());

    axum::serve(listener, app).await?;
    Ok(())
}
