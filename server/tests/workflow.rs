use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use bytes::{Buf, BytesMut};
use hecs::World;
use protocol::{Message, Type};
use server::*;
use tokio::io::*;
use tokio::sync::Mutex;
use tokio::time::timeout;

// (module
//   (func (export "run") (param i32 i32) (result i32)
//     (local.get 0)
//     (local.get 1)
//     (i32.add)
//   )
// )
const TEST_MODULE: &'static [u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x72, 0x75, 0x6e, 0x00, 0x00, 0x0a, 0x09,
    0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
];

async fn send_message(stream: Arc<Mutex<DuplexStream>>, msg: &Message) {
    let data = msg.encode().unwrap();
    let mut locked = stream.lock().await;
    locked.write_all(&data).await.unwrap();
    locked.flush().await.unwrap();
}

async fn receive_message(stream: Arc<Mutex<DuplexStream>>, bytes: &mut BytesMut) -> Option<Message> {
    let n = {
        let mut locked = stream.lock().await;
        match locked.read_buf(bytes).await {
            Ok(n) => n,
            _ => return None,
        }
    };

    if n == 0 {
        return None; // EOF
    }

    if let Ok((msg, consumed)) = Message::decode(&bytes) {
        bytes.advance(consumed);
        Some(msg)
    } else {
        None
    }
}

async fn run_client(stream: Arc<Mutex<DuplexStream>>) {
    let ready_msg = Message::ClientReady {
        module_name: None,
        device_ram: 1024 * 8,
    };
    send_message(stream.clone(), &ready_msg).await;

    let task_msg = timeout(Duration::from_secs(1), async {
        let mut bytes = BytesMut::new();

        loop {
            if let Some(msg) = receive_message(stream.clone(), &mut bytes).await {
                return msg;
            }
        }
    })
    .await
    .unwrap();

    if let Message::ServerTask { task_id, module, .. } = task_msg {
        assert_eq!(module.name, "test");
        assert_eq!(module.chunk_size, 16);
        assert_eq!(module.total_chunks, 3);

        for idx in 0..module.total_chunks {
            let ack_msg = Message::ClientAck {
                task_id,
                chunk_index: Some(idx),
                success: true,
            };
            send_message(stream.clone(), &ack_msg).await;
        }

        let result_msg = Message::ClientResult {
            task_id,
            result: vec![Type::I32(30)],
        };
        send_message(stream.clone(), &result_msg).await;
    } else {
        panic!("Fail to get message");
    };
}

async fn run_server(stream: Arc<Mutex<DuplexStream>>) {
    let mut world = World::new();

    let task_entity = world.spawn((
        Task {
            module_name: "test".into(),
            module_binary: TEST_MODULE.to_vec(),
            params: vec![Type::I32(10), Type::I32(20)],
            result: vec![],
            created_at: SystemTime::now(),
            chunk_size: 16,
            total_chunks: 3,
            priority: 1,
        },
        TaskState {
            phase: TaskPhase::Queued,
            deadline: None,
        },
    ));

    world.spawn((
        Session {
            device_addr: "0.0.0.0:0".parse().unwrap(),
            device_ram: 0,
            message_queue: VecDeque::new(),
            latency: Duration::default(),
        },
        SessionStream {
            inner: stream.clone(),
            incoming: BytesMut::new(),
            outgoing: BytesMut::new(),
        },
        SessionHealth {
            retries: 0,
            status: SessionStatus::Connected,
            last_heartbeat: SystemTime::now(),
        },
    ));

    for _ in 0..10 {
        LifecycleSystem::maintain_connection(&mut world).await;
        NetworkSystem::process_inbound::<DuplexStream>(&mut world).await;
        TaskSystem::assign_tasks(&mut world);
        TaskSystem::distribute_chunks(&mut world);
        NetworkSystem::process_outbound::<DuplexStream>(&mut world).await;
        tokio::time::sleep(Duration::from_millis(10)).await;

        if let Ok((task, state, transfer)) = world.query_one_mut::<(&Task, &mut TaskState, &TaskTransfer)>(task_entity) {
            match state.phase {
                TaskPhase::Distributing => {
                    let sent_chunks = transfer.acked_chunks.count_ones();
                    assert!(sent_chunks <= 3);
                }
                TaskPhase::Executing => {
                    assert!(transfer.acked_chunks.all());
                }
                TaskPhase::Completed => {
                    assert_eq!(task.result, vec![Type::I32(30)]);
                }
                _ => {}
            }
        }
    }
}

#[tokio::test]
async fn test_workflow() {
    let (server_conn, client_conn) = duplex(1024);

    let server_handle = tokio::spawn(run_server(Arc::new(Mutex::new(server_conn))));
    let client_handle = tokio::spawn(run_client(Arc::new(Mutex::new(client_conn))));

    let (server_res, client_res) = tokio::join!(server_handle, client_handle);
    
    server_res.unwrap();
    client_res.unwrap();
}
