mod common;

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use common::{TestClient, TestServer};
use hecs::Entity;
use protocol::{AckInfo, Message, Type};
use server::*;
use tokio::io::*;
use tokio::task::JoinSet;

// (module
//   (func (export "run") (param i32 i32) (result i32)
//     (local.get 0)
//     (local.get 1)
//     (i32.add)
//   )
// )
const TEST_MODULE: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x72, 0x75, 0x6e, 0x00, 0x00, 0x0a, 0x09,
    0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
];

async fn run_client(streams: Vec<DuplexStream>) {
    async fn process_client(client: &mut TestClient<DuplexStream>) {
        let mut cached: Option<String> = None;
        loop {
            let task_msg = client
                .receive(Some(Duration::from_millis(1)))
                .await
                .unwrap();

            if let Message::ServerTask { task_id, module, params } = task_msg {
                let ack_msg = Message::ClientAck {
                    task_id,
                    ack_info: AckInfo::Module {
                        modules: cached.as_ref().map_or(Vec::new(), |v| vec![v.clone()]),
                    },
                };
                client.send(&ack_msg).await.unwrap();

                if cached.as_ref().is_none_or(|name| name != &module.name) {
                    for idx in 0..module.total_chunks {
                        client
                            .receive(Some(Duration::from_millis(1)))
                            .await
                            .unwrap();

                        let ack_msg = Message::ClientAck {
                            task_id,
                            ack_info: AckInfo::Chunk {
                                chunk_index: idx,
                                success: true,
                            },
                        };
                        client.send(&ack_msg).await.unwrap();
                    }
                    cached = Some(module.name.clone());
                }

                let result = params.iter().fold(0, |acc, x| match x {
                    Type::I32(x) => acc + x,
                    _ => acc,
                });
                let result_msg = Message::ClientResult {
                    task_id,
                    result: vec![Type::I32(result)],
                };
                client.send(&result_msg).await.unwrap();

                let ack_msg = client
                    .receive(Some(Duration::from_millis(1)))
                    .await
                    .unwrap();
                assert!(matches!(ack_msg, Message::ServerAck { success: true, .. }));
            }
        }
    }

    let mut jobs = JoinSet::new();

    for stream in streams {
        jobs.spawn(async move {
            let mut client = TestClient::new(stream);
            client.handshake(Vec::new(), 1024 * 8).await.unwrap();
            process_client(&mut client).await;
        });
    }

    jobs.join_all().await;
}

async fn run_server(streams: Vec<DuplexStream>, module_count: usize, task_count: usize) {
    let mut server = TestServer::new();

    for stream in streams {
        server.add_session(stream);
    }

    let modules: Vec<Entity> = (0..module_count)
        .map(|i| {
            server.add_module(Module {
                name: format!("module_{}", i),
                binary: TEST_MODULE.to_vec(),
                dependencies: vec![],
                chunk_size: 16,
            })
        })
        .collect();

    let task_entities: Vec<Entity> = (0..task_count)
        .map(|i| {
            server.add_task(Task {
                name: format!("task_{}", i),
                params: vec![Type::I32(i as i32 * 10), Type::I32((i + 1) as i32 * 10)],
                result: vec![],
                created_at: SystemTime::now(),
                require_module: *modules.get(i % module_count).unwrap(),
                priority: 1,
            })
        })
        .collect();

    let mut completed = HashMap::new();
    loop {
        server.process_lifecycle::<DuplexStream>().await;

        for entity in &task_entities {
            if let Ok(state) = server.world.get::<&TaskState>(*entity) {
                if matches!(state.phase, TaskStatePhase::Completed) {
                    completed.insert(entity, true);
                }
            }
        }

        if completed.len() == task_count && completed.iter().all(|(_, v)| *v) {
            break;
        }
    }

    let sessions = server.world.query::<&SessionHealth>().iter().count();
    assert_eq!(sessions, 2);
}

#[tokio::test]
async fn test_multi_sessions() {
    let (server_conn1, client_conn1) = duplex(1024);
    let (server_conn2, client_conn2) = duplex(1024);

    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .try_init()
        .unwrap();

    let mut server_handle = tokio::spawn(run_server(vec![server_conn1, server_conn2], 2, 10));
    let mut client_handle = tokio::spawn(run_client(vec![client_conn1, client_conn2]));

    tokio::select! {
        _ = &mut server_handle => {
            client_handle.abort();
        }
        _ = &mut client_handle => {
            server_handle.abort();
        }
    }
}
