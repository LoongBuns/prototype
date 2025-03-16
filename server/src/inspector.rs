use std::error::Error;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use hecs::World;
use log::info;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;

use crate::components::*;

#[derive(serde::Serialize)]
pub struct SessionInfo {
    pub address: String,
    pub ram_mb: f32,
    pub latency_ms: u128,
    pub status: String,
    pub modules: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct TaskProgress {
    pub total: u32,
    pub completed: u32,
}

#[derive(serde::Serialize)]
pub struct TaskInfo {
    pub id: u64,
    pub module: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<TaskProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Vec<String>>,
}

#[derive(Clone)]
struct AppState {
    world: Arc<Mutex<World>>,
}

async fn handle_sessions(State(state): State<AppState>) -> Json<Vec<SessionInfo>> {
    let world = state.world.lock().await;

    let sessions = world
        .query::<(&Session, &SessionHealth)>()
        .iter()
        .map(|(_, (session, health))| SessionInfo {
            address: session.device_addr.to_string(),
            ram_mb: session.device_ram as f32 / 1024.0 / 1024.0,
            latency_ms: session.latency.as_millis(),
            status: format!("{:?}", health.status),
            modules: session.modules.iter().cloned().collect(),
        })
        .collect::<Vec<_>>();

    Json(sessions)
}

async fn handle_tasks(State(state): State<AppState>) -> Json<Vec<TaskInfo>> {
    let world = state.world.lock().await;

    let tasks = world
        .query::<(&Task, &TaskState, Option<&TaskTransfer>)>()
        .iter()
        .map(|(entity, (task, state, transfer))| {
            let progress = transfer.map(|tr| TaskProgress {
                total: tr.acked_chunks.len() as u32,
                completed: tr.acked_chunks.count_ones() as u32,
            });

            TaskInfo {
                id: entity.to_bits().into(),
                module: task.module_name.clone(),
                state: format!("{:?}", state.phase),
                progress,
                result: Some(task.result.iter().map(|t| format!("{:?}", t)).collect()),
            }
        })
        .collect::<Vec<_>>();

    Json(tasks)
}

pub async fn run(world: &Arc<Mutex<World>>, addr: &str) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(addr).await?;
    info!("Inspector server listening on: {}", listener.local_addr()?);

    let app_state = AppState {
        world: world.clone(),
    };

    let app = Router::new()
        .route("/api/sessions", get(handle_sessions))
        .route("/api/tasks", get(handle_tasks))
        .with_state(app_state)
        .layer(CorsLayer::permissive());

    axum::serve(listener, app).await?;
    Ok(())
}
