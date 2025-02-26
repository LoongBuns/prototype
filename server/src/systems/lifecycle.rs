use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::BytesMut;
use hecs::World;
use protocol::Message;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::components::*;

pub struct LifecycleSystem;

impl LifecycleSystem {
    const MAX_RETRIES: u8 = 5;
    const TIMEOUT: Duration = Duration::from_secs(30);

    pub async fn accept(world: &mut World, listener: &TcpListener) {
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

    pub fn maintain_health(world: &mut World) {
        let mut disconnected = Vec::new();
        let now = SystemTime::now();

        for (entity, (session, health)) in world.query::<(&mut Session, &mut SessionHealth)>().iter() {
            session.read_buffer.retain(|message| match message {
                Message::Heartbeat { timestamp } => {
                    health.last_heartbeat = SystemTime::now();

                    let last_record = UNIX_EPOCH + Duration::from_nanos(*timestamp);
                    let latency = now.duration_since(last_record).unwrap();
                    session.latency = latency;

                    false
                }
                _ => true,
            });

            let duration = now.duration_since(health.last_heartbeat).unwrap();

            match health.status {
                SessionStatus::Connected if duration > Self::TIMEOUT => {
                    health.status = SessionStatus::Zombie;
                    health.retries = 0;
                }
                SessionStatus::Zombie => {
                    health.retries += 1;
                    if health.retries > Self::MAX_RETRIES {
                        disconnected.push(entity);
                    }
                }
                _ => {}
            }
        }

        for entity in disconnected {
            world.despawn(entity).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use hecs::World;

    use super::*;

    #[tokio::test]
    async fn test_session_timeout() {
        let mut world = World::new();

        let entity = world.spawn((
            Session {
                device_addr: "0.0.0.0:0".parse().unwrap(),
                device_ram: 1,
                read_buffer: VecDeque::new(),
                write_buffer: VecDeque::new(),
                latency: Duration::from_millis(30),
            },
            SessionHealth {
                retries: 0,
                status: SessionStatus::Connected,
                last_heartbeat: SystemTime::now() - Duration::from_secs(31),
            },
        ));

        LifecycleSystem::maintain_health(&mut world);

        let updated_health = world.get::<&SessionHealth>(entity).unwrap();
        assert_eq!(updated_health.status, SessionStatus::Zombie);
    }
}
