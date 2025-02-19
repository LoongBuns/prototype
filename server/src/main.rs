mod loader;

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use loader::get_wasm_modules;
use protocol::{Config, Message, Type};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Queued,
    Dispatched,
    Completed,
    Failed,
}

pub struct Task {
    pub wasm_binary: Vec<u8>,
    pub params: Vec<Type>,
    pub status: TaskStatus,
}

#[derive(Clone)]
struct ServerState {
    task_queue: Arc<Mutex<VecDeque<Task>>>,
    pending_tasks: Arc<Mutex<HashMap<u64, Task>>>,
    next_task_id: Arc<AtomicU64>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            task_queue: Arc::new(Mutex::new(VecDeque::new())),
            pending_tasks: Arc::new(Mutex::new(HashMap::new())),
            next_task_id: Arc::new(AtomicU64::new(1)),
        }
    }

    async fn generate(&self) {
        let mut queue = self.task_queue.lock().await;
        let modules = get_wasm_modules();

        for module in modules.iter() {
            match module.name {
                "render" => {
                    const HEIGHT: i32 = 300;
                    const CHUNK_SIZE: i32 = 100;

                    for start_row in (0..HEIGHT).step_by(CHUNK_SIZE as usize) {
                        let end_row = (start_row + CHUNK_SIZE).min(HEIGHT);

                        queue.push_back(Task {
                            wasm_binary: module.data.to_vec(),
                            params: vec![Type::I32(start_row), Type::I32(end_row)],
                            status: TaskStatus::Queued,
                        });
                    }
                }
                "fiber" => {
                    queue.push_back(Task {
                        wasm_binary: module.data.to_vec(),
                        params: vec![],
                        status: TaskStatus::Queued,
                    });
                }
                _ => {}
            }
        }
    }
}

async fn handle_connection(mut socket: TcpStream, state: Arc<ServerState>) {
    let mut buf = [0u8; 1024];

    loop {
        let n = socket.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }

        match Message::deserialize(&buf[..n]) {
            Ok(Message::ClientReady) => {
                let task = {
                    let mut queue = state.task_queue.lock().await;
                    queue.pop_front()
                };

                if let Some(mut task) = task {
                    task.status = TaskStatus::Dispatched;

                    let task_id = state.next_task_id.fetch_add(1, Ordering::SeqCst);

                    let msg = Message::ServerTask {
                        task_id,
                        binary: task.wasm_binary.clone(),
                        params: task.params.clone(),
                    };

                    socket.write_all(&msg.serialize()).await.unwrap();

                    state.pending_tasks.lock().await.insert(task_id, task);
                }
            }
            Ok(Message::ClientResult { task_id, result }) => {
                let state_clone = state.clone();
                process_result(state_clone, task_id, &result).await;

                let ack = Message::ServerAck {
                    task_id,
                    success: true,
                };
                socket.write_all(&ack.serialize()).await.unwrap();

                state.pending_tasks.lock().await.remove(&task_id);
            }
            _ => {}
        }
    }
}

async fn process_result(state: Arc<ServerState>, task_id: u64, result: &[Type]) {
    let mut pending_tasks = state.pending_tasks.lock().await;
    if let Some(mut task) = pending_tasks.remove(&task_id) {
        task.status = if !result.is_empty() {
            TaskStatus::Completed
        } else {
            TaskStatus::Failed
        };
    }
}

#[tokio::main]
async fn main() {
    let Config { host, port, .. } = Config::new();
    let addr = format!("{}:{}", host, port);

    let listener = TcpListener::bind(&addr).await.unwrap();
    let state = Arc::new(ServerState::new());
    state.generate().await;

    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let state_clone = state.clone();
        tokio::spawn(handle_connection(stream, state_clone));
    }
}
