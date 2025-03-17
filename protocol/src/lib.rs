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
pub struct ModuleInfo {
    pub name: String,
    pub size: u64,
    pub chunk_size: u32,
    pub total_chunks: u32,
}

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, PartialEq)]
pub enum AckInfo {
    Chunk {
        chunk_index: u32,
        success: bool,
    },
    Module {
        modules: Vec<String>,
    },
}

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, PartialEq)]
pub enum Message {
    ClientReady {
        modules: Vec<String>,
        device_ram: u64,
    },
    ServerTask {
        task_id: u64,
        module: ModuleInfo,
        params: Vec<Type>,
    },
    ServerModule {
        task_id: u64,
        chunk_index: u32,
        chunk_data: Vec<u8>,
    },
    ClientAck {
        task_id: u64,
        ack_info: AckInfo,
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
    pub const HEADER_SIZE: usize = 2;

    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        let config = bincode::config::standard()
            .with_variable_int_encoding()
            .with_big_endian();
        let payload = bincode::encode_to_vec(self, config).map_err(Error::EncodeError)?;
        let payload_len = payload.len();

        if payload_len > u16::MAX as usize {
            return Err(Error::InvalidMessage);
        }

        let mut output = Vec::with_capacity(Self::HEADER_SIZE + payload_len);
        output.extend_from_slice(&(payload_len as u16).to_be_bytes());
        output.extend(payload);

        Ok(output)
    }

    pub fn decode(data: &[u8]) -> Result<(Self, usize), Error> {
        if data.len() < Self::HEADER_SIZE {
            return Err(Error::InsufficientData);
        }

        let payload_len = u16::from_be_bytes([data[0], data[1]]) as usize;
        let total_len = Self::HEADER_SIZE + payload_len;

        if data.len() < total_len {
            return Err(Error::InsufficientData);
        }

        let config = bincode::config::standard()
            .with_variable_int_encoding()
            .with_big_endian();

        let (message, size) =
            bincode::decode_from_slice(&data[Self::HEADER_SIZE..total_len], config)
                .map_err(Error::DecodeError)?;

        if size != payload_len {
            return Err(Error::InvalidMessage);
        }

        Ok((message, total_len))
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn test_client_ready() {
        let msg = Message::ClientReady {
            modules: vec!["test".into()],
            device_ram: 0,
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg, decoded.0);
    }

    #[test]
    fn test_server_task() {
        let msg = Message::ServerTask {
            task_id: 99,
            module: ModuleInfo {
                name: "test".into(),
                size: 1024,
                chunk_size: 256,
                total_chunks: 4,
            },
            params: vec![
                Type::Void,
                Type::I32(-123),
                Type::F32(core::f32::consts::PI),
                Type::I64(987_654_321),
                Type::F64(core::f64::consts::E),
                Type::V128(123456789012345678901234567890),
            ],
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg, decoded.0);
    }

    #[test]
    fn test_server_module() {
        let msg = Message::ServerModule {
            task_id: 99,
            chunk_index: 1,
            chunk_data: vec![10, 20, 30, 40, 50],
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg, decoded.0);
    }

    #[test]
    fn test_client_ack() {
        let msg_success = Message::ClientAck {
            task_id: 99,
            ack_info: AckInfo::Module {
                modules: vec!["test".into()],
            },
        };
        let encoded = msg_success.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg_success, decoded.0);
    }

    #[test]
    fn test_client_result() {
        let msg = Message::ClientResult {
            task_id: 99,
            result: vec![Type::I32(42), Type::F64(-5.67)],
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg, decoded.0);
    }

    #[test]
    fn test_server_ack() {
        let msg_success = Message::ServerAck {
            task_id: 1,
            success: true,
        };
        let encoded = msg_success.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg_success, decoded.0);
    }

    #[test]
    fn test_heartbeat() {
        let msg = Message::Heartbeat {
            timestamp: 1234567890,
        };
        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();
        assert_eq!(msg, decoded.0);
    }

    #[test]
    fn test_encode_invalid_message() {
        let long_string = "a".repeat(u16::MAX as usize + 1);
        let msg = Message::ClientReady {
            modules: vec![long_string],
            device_ram: 0,
        };
        let result = msg.encode();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InvalidMessage));
    }

    #[test]
    fn test_decode_insufficient_data_header() {
        let data = vec![1];
        let result = Message::decode(&data);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InsufficientData));
    }

    #[test]
    fn test_decode_insufficient_data_payload() {
        let data = vec![0, 5, 1, 2, 3];
        let result = Message::decode(&data);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::InsufficientData));
    }

    #[test]
    fn test_decode_decode_error() {
        let msg = Message::ClientReady {
            modules: Vec::new(),
            device_ram: 0,
        };
        let mut encoded = msg.encode().unwrap();
        if encoded.len() > 2 {
            encoded[2] = encoded[2].wrapping_add(1);
        }
        let result = Message::decode(&encoded);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::DecodeError(_)));
    }
}
