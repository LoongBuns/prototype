use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use protocol::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::time::timeout;

pub struct TestClient<T> {
    pub conn: Arc<Mutex<T>>,
}

impl<T> TestClient<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    pub fn new(stream: T) -> Self {
        Self {
            conn: Arc::new(Mutex::new(stream)),
        }
    }

    pub async fn send(&mut self, msg: &Message) -> Result<(), Box<dyn Error>> {
        let data = msg.encode()?;
        let mut conn = self.conn.lock().await;
        conn.write_all(&data).await?;
        conn.flush().await?;
        Ok(())
    }

    pub async fn receive(&mut self, timeout_duration: Option<Duration>) -> Result<Message, Box<dyn Error>> {
        async fn read_message<T>(client: &TestClient<T>) -> Result<Message, Box<dyn Error>>
        where
            T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
        {
            let mut conn = client.conn.lock().await;

            let mut header = [0u8; Message::HEADER_SIZE];
            conn.read_exact(&mut header).await?;

            let payload_len = u16::from_be_bytes(header) as usize;
            let total_len = Message::HEADER_SIZE + payload_len;

            let mut buffer = vec![0u8; total_len];
            buffer[..Message::HEADER_SIZE].copy_from_slice(&header);

            conn.read_exact(&mut buffer[Message::HEADER_SIZE..]).await?;

            let (msg, consumed) = Message::decode(&buffer)?;
            assert_eq!(consumed, total_len);
            Ok(msg)
        }

        match timeout_duration {
            Some(duration) => {
                Ok(timeout(duration, async {
                    loop {
                        if let Ok(msg) = read_message(self).await {
                            return msg;
                        }
                    }
                })
                .await?)
            }
            None => read_message(self).await,
        }
    }

    pub async fn handshake(&mut self, module: Option<String>, ram: u64) -> Result<(), Box<dyn Error>> {
        self.send(&Message::ClientReady {
            module_name: module,
            device_ram: ram,
        })
        .await
    }
}
