mod loader;

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use loader::{get_wasm_modules, WasmModule};
use protocol::{Config, Message, ModuleMeta, Type};
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
    pub module_meta: ModuleMeta,
    pub wasm_binary: Vec<u8>,
    pub params: Vec<Type>,
    pub status: TaskStatus,
}

impl Task {
    const CHUNK_SIZE: usize = 1024;

    pub fn new(module: &WasmModule, params: &[Type]) -> Self {
        let total_chunks = (module.data.len() + Self::CHUNK_SIZE - 1) / Self::CHUNK_SIZE;

        let module_meta = ModuleMeta {
            name: module.name.to_string(),
            size: module.data.len() as u64,
            chunk_size: Self::CHUNK_SIZE as u32,
            total_chunks: total_chunks as u32,
        };

        Self {
            module_meta,
            wasm_binary: module.data.to_vec(),
            params: params.to_vec(),
            status: TaskStatus::Queued,
        }
    }
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

                        let task = Task::new(&module, &[Type::I32(start_row), Type::I32(end_row)]);
                        queue.push_back(task);
                    }
                }
                "fiber" => {
                    let task = Task::new(&module, &[]);
                    queue.push_back(task);
                }
                _ => {}
            }
        }
    }
}

async fn handle_connection(mut socket: TcpStream, state: Arc<ServerState>) -> Result<(), Box<dyn Error>> {
    let mut buf = [0u8; 1024];

    loop {
        let n = socket.read(&mut buf).await?;
        if n == 0 {
            break;
        }

        match Message::decode(&buf[..n])? {
            Message::Heartbeat { timestamp } => {
                let task = {
                    let mut queue = state.task_queue.lock().await;
                    queue.pop_front()
                };

                if let Some(mut task) = task {
                    task.status = TaskStatus::Dispatched;

                    let task_id = state.next_task_id.fetch_add(1, Ordering::SeqCst);

                    let msg = Message::ServerTask {
                        task_id,
                        module: task.module_meta,
                        params: task.params.clone(),
                    };
                    socket.write_all(&msg.encode()?).await?;

                    let chunks = task.wasm_binary.chunks(task.module_meta.chunk_size as usize);
                    for (index, chunk) in chunks.enumerate() {
                        let chunk_msg = Message::ServerModuleChunk {
                            task_id,
                            chunk_index: index as u32,
                            chunk_data: chunk.to_vec(),
                        };
                        socket.write_all(&chunk_msg.encode()?).await?;
                    }

                    state.pending_tasks.lock().await.insert(task_id, task);
                }
            }
            Message::ClientResult { task_id, result } => {
                let state_clone = state.clone();
                process_result(state_clone, task_id, &result).await;

                let ack = Message::ServerAck {
                    task_id,
                    success: true,
                };
                socket.write_all(&ack.encode()?).await?;

                state.pending_tasks.lock().await.remove(&task_id);
            }
            _ => {}
        }
    }

    Ok(())
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
