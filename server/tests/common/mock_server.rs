use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::BytesMut;
use hecs::{Entity, World};
use server::*;
use tokio::io::*;
use tokio::sync::Mutex;

pub struct TestServer {
    pub world: World,
}

impl TestServer {
    pub fn new() -> Self {
        Self {
            world: World::new(),
        }
    }

    pub fn add_task(&mut self, task: Task) -> Entity {
        self.world.spawn((
            task,
            TaskState {
                phase: TaskStatePhase::Queued,
                deadline: None,
                assigned_device: None,
            },
        ))
    }

    pub fn add_session<T>(&mut self, stream: T) -> Entity
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        self.world.spawn((
            Session {
                device_addr: "0.0.0.0:0".parse().unwrap(),
                device_ram: 0,
                message_queue: VecDeque::new(),
                latency: Duration::default(),
                cached_modules: HashSet::new(),
            },
            SessionStream {
                inner: Arc::new(Mutex::new(stream)),
                incoming: BytesMut::new(),
                outgoing: BytesMut::new(),
            },
            SessionHealth {
                retries: 0,
                status: SessionStatus::Connected,
                last_heartbeat: SystemTime::now(),
            },
        ))
    }

    pub async fn process_lifecycle<T>(&mut self)
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        NetworkSystem::process_inbound::<T>(&mut self.world).await;
        TaskSystem::assign_tasks(&mut self.world);
        TaskSystem::distribute_chunks(&mut self.world);
        NetworkSystem::process_outbound::<T>(&mut self.world).await;
    }
}
