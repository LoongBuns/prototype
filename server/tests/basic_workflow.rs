mod common;

use std::time::{Duration, SystemTime};

use common::{TestClient, TestServer};
use protocol::{AckInfo, Message, Type};
use server::*;
use tokio::io::*;

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

async fn run_client(stream: DuplexStream) {
    let mut client = TestClient::new(stream);
    client.handshake(Vec::new(), 1024 * 8).await.unwrap();

    let task_msg = client
        .receive(Some(Duration::from_millis(1)))
        .await
        .unwrap();

    if let Message::ServerTask { task_id, module, .. } = task_msg {
        assert_eq!(module.name, "test");
        assert_eq!(module.chunk_size, 16);
        assert_eq!(module.total_chunks, 3);

        let ack_msg = Message::ClientAck {
            task_id,
            ack_info: AckInfo::Task {
                modules: vec![],
            },
        };
        client.send(&ack_msg).await.unwrap();

        for idx in 0..module.total_chunks {
            let chunk_msg = client
                .receive(Some(Duration::from_millis(1)))
                .await
                .unwrap();
            assert!(matches!(
                chunk_msg,
                Message::ServerModule { chunk_index, .. }
                if chunk_index == idx
            ));

            let ack_msg = Message::ClientAck {
                task_id,
                ack_info: AckInfo::Module {
                    chunk_index: idx,
                    success: true,
                },
            };
            client.send(&ack_msg).await.unwrap();
        }

        let result_msg = Message::ClientResult {
            task_id,
            result: vec![Type::I32(30)],
        };
        client.send(&result_msg).await.unwrap();

        let ack_msg = client
            .receive(Some(Duration::from_millis(1)))
            .await
            .unwrap();
        assert!(matches!(ack_msg, Message::ServerAck { success: true, .. }));
    } else {
        panic!("Fail to get message");
    };
}

async fn run_server(stream: DuplexStream) {
    let mut server = TestServer::new();
    server.add_session(stream);
    let task_entity = server.add_task(Task {
        module_name: "test".into(),
        module_binary: TEST_MODULE.to_vec(),
        params: vec![Type::I32(10), Type::I32(20)],
        result: vec![],
        created_at: SystemTime::now(),
        chunk_size: 16,
        total_chunks: 3,
        priority: 1,
    });

    loop {
        server.process_lifecycle::<DuplexStream>().await;

        if let Ok((task, state)) = server.world.query_one_mut::<(&Task, &TaskState)>(task_entity) {
            if matches!(state.phase, TaskStatePhase::Completed) {
                assert_eq!(task.result, vec![Type::I32(30)]);
                break;
            }
        }
    }
}

#[tokio::test]
async fn test_workflow() {
    let (server_conn, client_conn) = duplex(1024);

    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .try_init()
        .unwrap();

    let server_handle = tokio::spawn(run_server(server_conn));
    let client_handle = tokio::spawn(run_client(client_conn));

    let (server_res, client_res) = tokio::join!(server_handle, client_handle);

    server_res.unwrap();
    client_res.unwrap();
}
