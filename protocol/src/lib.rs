#![no_std]

extern crate alloc;

mod config;

use alloc::vec::Vec;

pub use config::{Config, Wifi};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid message type: {0}")]
    InvalidMessageType(u8),
    #[error("Insufficient data")]
    InsufficientData,
}

#[derive(Debug, Clone)]
pub enum Message {
    ClientReady,
    ServerTask {
        task_id: u64,
        binary: Vec<u8>,
        params: Vec<u8>,
    },
    ClientResult {
        task_id: u64,
        result: Vec<u8>,
    },
    ServerAck {
        task_id: u64,
        success: bool,
    },
}

impl Message {
    const HEADER_SIZE: usize = 3;

    pub fn serialize(&self) -> Vec<u8> {
        let (msg_type, payload) = match self {
            Message::ClientReady => (0x01, Vec::new()),
            Message::ServerTask {
                task_id,
                binary,
                params,
            } => {
                let mut payload = Vec::new();
                payload.extend_from_slice(&task_id.to_be_bytes());

                payload.extend_from_slice(&(binary.len() as u32).to_be_bytes());
                payload.extend_from_slice(binary);

                payload.extend_from_slice(params);
                (0x02, payload)
            }
            Message::ClientResult { task_id, result } => {
                let mut payload = Vec::new();
                payload.extend_from_slice(&task_id.to_be_bytes());

                payload.extend_from_slice(&(result.len() as u16).to_be_bytes());
                payload.extend_from_slice(result);
                (0x03, payload)
            }
            Message::ServerAck { task_id, success } => {
                let mut payload = Vec::new();
                payload.extend_from_slice(&task_id.to_be_bytes());
                payload.push(*success as u8);
                (0x04, payload)
            }
        };

        let mut buf = Vec::with_capacity(Self::HEADER_SIZE + payload.len());
        buf.push(msg_type);
        buf.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        buf.extend(payload);
        buf
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, Error> {
        if data.len() < Self::HEADER_SIZE {
            return Err(Error::InsufficientData);
        }

        let msg_type = data[0];
        let payload_len = u16::from_be_bytes([data[1], data[2]]) as usize;
        let payload = &data[Self::HEADER_SIZE..Self::HEADER_SIZE + payload_len];

        match msg_type {
            0x01 => Ok(Message::ClientReady),
            0x02 => {
                if payload.len() < 12 {
                    return Err(Error::InsufficientData);
                }

                let task_id = u64::from_be_bytes(payload[0..8].try_into().unwrap());

                let binary_len = u32::from_be_bytes(payload[8..12].try_into().unwrap()) as usize;
                let binary = payload[12..12 + binary_len].to_vec();
                let params_start = 12 + binary_len;
                let params = payload[params_start..].to_vec();

                Ok(Message::ServerTask {
                    task_id,
                    binary,
                    params,
                })
            }
            0x03 => {
                if payload.len() < 10 {
                    return Err(Error::InsufficientData);
                }

                let task_id = u64::from_be_bytes(payload[0..8].try_into().unwrap());
                let result_len = u16::from_be_bytes([payload[8], payload[9]]) as usize;

                if payload[10..].len() < result_len {
                    return Err(Error::InsufficientData);
                }
                let result = payload[10..10 + result_len].to_vec();

                Ok(Message::ClientResult { task_id, result })
            }
            0x04 => {
                if payload.len() < 9 {
                    return Err(Error::InsufficientData);
                }

                let task_id = u64::from_be_bytes(payload[0..8].try_into().unwrap());
                let success = payload[8] != 0;

                Ok(Message::ServerAck { task_id, success })
            }
            _ => Err(Error::InvalidMessageType(msg_type)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_ready() {
        let message = Message::ClientReady;
        let serialized = message.serialize();
        let deserialized = Message::deserialize(&serialized).unwrap();
        match deserialized {
            Message::ClientReady => (),
            _ => panic!("Deserialized message type mismatch"),
        }
    }

    #[test]
    fn test_server_task() {
        let task_id = 0x123456789ABCDEF0;
        let binary = b"hello";
        let params = b"world";

        let message = Message::ServerTask {
            task_id,
            binary: binary.to_vec(),
            params: params.to_vec(),
        };

        let serialized = message.serialize();
        let deserialized = Message::deserialize(&serialized).unwrap();
        match deserialized {
            Message::ServerTask {
                task_id: received_task_id,
                binary: received_binary,
                params: received_params,
            } => {
                assert_eq!(received_task_id, task_id);
                assert_eq!(received_binary, binary);
                assert_eq!(received_params, params);
            }
            _ => panic!("Deserialized message type mismatch"),
        }
    }

    #[test]
    fn test_client_result() {
        let task_id = 0x123456789ABCDEF1;
        let result = b"success";

        let message = Message::ClientResult {
            task_id,
            result: result.to_vec(),
        };

        let serialized = message.serialize();
        let deserialized = Message::deserialize(&serialized).unwrap();
        match deserialized {
            Message::ClientResult {
                task_id: received_task_id,
                result: received_result,
            } => {
                assert_eq!(received_task_id, task_id);
                assert_eq!(received_result, result);
            }
            _ => panic!("Deserialized message type mismatch"),
        }
    }

    #[test]
    fn test_server_ack() {
        let task_id = 0x123456789ABCDEF2;
        let success = true;

        let message = Message::ServerAck { task_id, success };

        let serialized = message.serialize();
        let deserialized = Message::deserialize(&serialized).unwrap();
        match deserialized {
            Message::ServerAck {
                task_id: received_task_id,
                success: received_success,
            } => {
                assert_eq!(received_task_id, task_id);
                assert_eq!(received_success, success);
            }
            _ => panic!("Deserialized message type mismatch"),
        }
    }

    #[test]
    fn test_invalid_message_type() {
        let data = [0x00, 0x00, 0x00];
        let result = Message::deserialize(&data);
        assert!(matches!(result, Err(Error::InvalidMessageType(_))));
    }

    #[test]
    fn test_insufficient_data() {
        let data = [0x01];
        let result = Message::deserialize(&data);
        assert!(matches!(result, Err(Error::InsufficientData)));
    }
}
