mod loader;
mod task;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use protocol::{Config, Message, Type};
use task::{Task, TaskStatus};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

#[derive(Clone)]
struct ServerState {
    task_queue: Arc<Mutex<Vec<Task>>>,
    pending_tasks: Arc<Mutex<HashMap<u64, Task>>>,
    next_task_id: Arc<AtomicU64>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            task_queue: Arc::new(Mutex::new(Vec::new())),
            pending_tasks: Arc::new(Mutex::new(HashMap::new())),
            next_task_id: Arc::new(AtomicU64::new(1)),
        }
    }

    async fn add_task(&self, task: Task) {
        let mut queue = self.task_queue.lock().await;
        queue.push(task);
    }
}

async fn handle_request(mut socket: TcpStream, state: Arc<ServerState>) {
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
                    queue.pop()
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

async fn process_result(state: Arc<ServerState>, task_id: u64, result: &Vec<Type>) {
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

    loop {
        let (socket, _) = listener.accept().await.unwrap();
        let state_clone = state.clone();
        tokio::spawn(async move {
            handle_request(socket, state_clone).await;
        });
    }
}
