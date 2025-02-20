#![no_std]

extern crate alloc;

mod config;

use alloc::string::String;
use alloc::vec::Vec;

pub use config::{Config, Wifi};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Insufficient data")]
    InsufficientData,
    #[error("Invalid message")]
    InvalidMessage,
    #[error("Decode error: {0:?}")]
    DecodeError(bincode::error::DecodeError),
    #[error("Encode error: {0:?}")]
    EncodeError(bincode::error::EncodeError),
}

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, PartialEq)]
pub enum Type {
    Void,
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128(i128),
}

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, PartialEq)]
pub struct ModuleMeta {
    pub name: String,
    pub size: u64,
    pub chunk_size: u32,
    pub total_chunks: u32,
}

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, PartialEq)]
pub enum Message {
    ClientReady {
        module_name: Option<String>,
        device_ram: u64,
    },
    ServerTask {
        task_id: u64,
        module: ModuleMeta,
        params: Vec<Type>,
    },
    ServerModuleChunk {
        task_id: u64,
        chunk_index: u32,
        chunk_data: Vec<u8>,
    },
    ClientResult {
        task_id: u64,
        result: Vec<Type>,
    },
    ServerAck {
        task_id: u64,
        success: bool,
    },
    Heartbeat {
        timestamp: u64,
    },
}

impl Message {
    const HEADER_SIZE: usize = 2;

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let payload = bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(Error::EncodeError)?;
        let payload_len = payload.len();

        if payload_len > u16::MAX as usize {
            return Err(Error::InvalidMessage);
        }

        let mut output = Vec::with_capacity(Self::HEADER_SIZE + payload_len);
        output.extend_from_slice(&(payload_len as u16).to_be_bytes());
        output.extend(payload);

        Ok(output)
    }

    pub fn decode(data: &[u8]) -> Result<Self, Error> {
        if data.len() < Self::HEADER_SIZE {
            return Err(Error::InsufficientData);
        }

        let payload_len = u16::from_be_bytes([data[0], data[1]]) as usize;

        let (message, size) =
            bincode::decode_from_slice(&data[Self::HEADER_SIZE..], bincode::config::standard())
                .map_err(Error::DecodeError)?;

        if size != payload_len {
            return Err(Error::InvalidMessage);
        }

        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn test_client_ready() {
        let msg = Message::Heartbeat { timestamp: 0 };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_server_task() {
        let msg = Message::ServerTask {
            task_id: 42,
            module: ModuleMeta {
                name: "test".into(),
                size: 1024,
                chunk_size: 256,
                total_chunks: 4,
            },
            params: vec![
                Type::Void,
                Type::I32(-123),
                Type::F32(3.14),
                Type::I64(987_654_321),
                Type::F64(2.718281828459045),
                Type::V128(123456789012345678901234567890),
            ],
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_server_module_chunk() {
        let msg = Message::ServerModuleChunk {
            task_id: 99,
            chunk_index: 1,
            chunk_data: vec![10, 20, 30, 40, 50],
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_client_result() {
        let msg = Message::ClientResult {
            task_id: 99,
            result: vec![Type::I32(42), Type::F64(-5.67)],
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_server_ack() {
        let msg_success = Message::ServerAck { task_id: 1, success: true };
        let encoded = msg_success.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg_success, decoded);
    }
}
