use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::BytesMut;
use hecs::World;
use log::{info, warn};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::components::*;

pub struct LifecycleSystem;

impl LifecycleSystem {
    const MAX_RETRIES: u8 = 5;
    const TIMEOUT: Duration = Duration::from_secs(32);

    pub fn accept_connection(world: &mut World, stream: TcpStream, addr: SocketAddr) {
        world.spawn((
            Session {
                message_queue: VecDeque::new(),
                modules: HashSet::new(),
                latency: Duration::default(),
            },
            SessionInfo {
                device_addr: addr,
                device_ram: 0,
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
        ));
    }

    pub async fn maintain_connection<T, F>(world: &mut World, callback: F)
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
        F: AsyncFn(SocketAddr) -> std::io::Result<T>,
    {
        let mut dead_sessions = Vec::new();
        let now = SystemTime::now();

        for (entity, (info, session, health)) in &mut world
            .query::<(&SessionInfo, &mut SessionStream<T>, &mut SessionHealth)>()
            .iter()
        {
            let elapsed = now
                .duration_since(health.last_heartbeat)
                .unwrap_or_default();

            match health.status {
                SessionStatus::Connected if elapsed > Self::TIMEOUT => {
                    warn!("Session {:?} timed out ({} secs), marked as zombie", entity, elapsed.as_secs());
                    health.status = SessionStatus::Zombie;
                    health.retries = 0;
                }
                SessionStatus::Zombie => {
                    health.retries += 1;
                    if health.retries >= Self::MAX_RETRIES {
                        info!("Session {:?} reached max retries, scheduled for removal", entity);
                        dead_sessions.push(entity);
                    }
                }
                SessionStatus::Disconnected => {
                    info!("Session {:?} disconnected, attempting reconnect", entity);
                    if let Ok(stream) = callback(info.device_addr).await {
                        info!("Session {:?} reconnected to {} successfully", entity, info.device_addr);
                        session.inner = Arc::new(Mutex::new(stream));
                        health.status = SessionStatus::Connected;
                        health.last_heartbeat = SystemTime::now();
                    }
                }
                _ => {}
            }
        }

        for entity in dead_sessions {
            world.despawn(entity).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use hecs::Entity;
    use tokio::io::SimplexStream;

    use super::*;

    fn create_mock_device<T>(world: &mut World, timeout: Duration, stream: &Arc<Mutex<T>>) -> Entity
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        world.spawn((
            SessionInfo {
                device_addr: "0.0.0.0:0".parse().unwrap(),
                device_ram: 1024,
            },
            SessionStream {
                inner: stream.clone(),
                incoming: BytesMut::new(),
                outgoing: BytesMut::new(),
            },
            SessionHealth {
                retries: 0,
                status: SessionStatus::Connected,
                last_heartbeat: SystemTime::now() - timeout,
            },
        ))
    }

    #[tokio::test]
    async fn test_maintain_connection() {
        let mut world = World::new();

        let device_entity = create_mock_device(
            &mut world,
            Duration::from_secs(33),
            &Arc::new(Mutex::new(SimplexStream::new_unsplit(1))),
        );

        async fn callback(_: SocketAddr) -> std::io::Result<SimplexStream> {
            Ok(SimplexStream::new_unsplit(1))
        }

        LifecycleSystem::maintain_connection(&mut world, callback).await;
        assert_eq!(
            world.get::<&SessionHealth>(device_entity).unwrap().status,
            SessionStatus::Zombie
        );

        for _ in 0..5 {
            LifecycleSystem::maintain_connection(&mut world, callback).await;
        }
        assert!(world.get::<&SessionHealth>(device_entity).is_err());
    }
}
