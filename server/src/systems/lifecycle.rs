use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::BytesMut;
use hecs::World;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::components::*;

pub struct LifecycleSystem;

impl LifecycleSystem {
    const MAX_RETRIES: u8 = 5;
    const TIMEOUT: Duration = Duration::from_secs(30);

    pub async fn accept_connection(world: &mut World, listener: &TcpListener) {
        while let Ok((stream, addr)) = listener.accept().await {
            world.spawn((
                Session {
                    device_addr: addr,
                    device_ram: 0,
                    message_queue: VecDeque::new(),
                    latency: Duration::default(),
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
    }

    pub async fn maintain_connection(world: &mut World) {
        let mut pending_reconnects = Vec::new();
        let mut dead_sessions = Vec::new();
        let now = SystemTime::now();

        for (entity, (session, health)) in world.query::<(&mut Session, &mut SessionHealth)>().iter() {
            let elapsed = now
                .duration_since(health.last_heartbeat)
                .unwrap_or_default();

            match health.status {
                SessionStatus::Connected if elapsed > Self::TIMEOUT => {
                    health.status = SessionStatus::Zombie;
                    health.retries = 0;
                }
                SessionStatus::Zombie => {
                    health.retries += 1;
                    if health.retries >= Self::MAX_RETRIES {
                        dead_sessions.push(entity);
                    }
                }
                SessionStatus::Disconnected => {
                    pending_reconnects.push((entity, session.device_addr));
                }
                _ => {}
            }
        }

        let mut reconnected = Vec::new();
        for (entity, addr) in pending_reconnects {
            if let Ok(stream) = TcpStream::connect(addr).await {
                reconnected.push((entity, stream));
            }
        }

        for entity in dead_sessions {
            world.despawn(entity).ok();
        }

        for (entity, stream) in reconnected {
            if let Ok((session, health)) = world.query_one_mut::<(&mut SessionStream<TcpStream>, &mut SessionHealth)>(entity) {
                session.inner = Arc::new(Mutex::new(stream));
                health.status = SessionStatus::Connected;
                health.last_heartbeat = SystemTime::now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::{Duration, SystemTime};

    use hecs::{Entity, World};

    use super::*;

    fn create_mock_device(world: &mut World, timeout: Duration) -> Entity {
        world.spawn((
            Session {
                device_addr: "0.0.0.0:0".parse().unwrap(),
                device_ram: 1024,
                message_queue: VecDeque::new(),
                latency: Duration::default(),
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
        let device_entity = create_mock_device(&mut world, Duration::from_secs(31));

        LifecycleSystem::maintain_connection(&mut world).await;
        assert_eq!(
            world.get::<&SessionHealth>(device_entity).unwrap().status,
            SessionStatus::Zombie
        );

        for _ in 0..5 {
            LifecycleSystem::maintain_connection(&mut world).await;
        }
        assert!(world.get::<&SessionHealth>(device_entity).is_err());
    }
}
