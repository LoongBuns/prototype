#![no_std]

#[macro_use]
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

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Void,
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128(i128),
}

impl Type {
    pub fn encode(&self) -> Vec<u32> {
        match *self {
            Type::Void => {
                vec![]
            }
            Type::I32(value) => {
                let in_u32_array = unsafe { core::mem::transmute::<i32, [u32; 1]>(value) };
                vec![in_u32_array[0]]
            }
            Type::I64(value) => {
                let in_u32_array = unsafe { core::mem::transmute::<i64, [u32; 2]>(value) };
                vec![in_u32_array[0], in_u32_array[1]]
            }
            Type::F32(value) => {
                let in_u32_array = unsafe { core::mem::transmute::<f32, [u32; 1]>(value) };
                vec![in_u32_array[0]]
            }
            Type::F64(value) => {
                let in_u32_array = unsafe { core::mem::transmute::<f64, [u32; 2]>(value) };
                vec![in_u32_array[0], in_u32_array[1]]
            }
            Type::V128(value) => {
                let in_u32_array = unsafe { core::mem::transmute::<i128, [u32; 4]>(value) };
                vec![in_u32_array[0], in_u32_array[1], in_u32_array[2], in_u32_array[3]]
            }
        }
    }

    pub fn decode_to_i32(binary: Vec<u32>) -> Type {
        let binary: [u32; 1] = [binary[0]];
        Type::I32(unsafe { core::mem::transmute::<[u32; 1], i32>(binary) })
    }

    pub fn decode_to_f32(binary: Vec<u32>) -> Type {
        let binary: [u32; 1] = [binary[0]];
        Type::F32(unsafe { core::mem::transmute::<[u32; 1], f32>(binary) })
    }

    pub fn decode_to_i64(binary: Vec<u32>) -> Type {
        let binary: [u32; 2] = [binary[0], binary[1]];
        Type::I64(unsafe { core::mem::transmute::<[u32; 2], i64>(binary) })
    }

    pub fn decode_to_f64(binary: Vec<u32>) -> Type {
        let binary: [u32; 2] = [binary[0], binary[1]];
        Type::F64(unsafe { core::mem::transmute::<[u32; 2], f64>(binary) })
    }

    pub fn decode_to_v128(binary: Vec<u32>) -> Type {
        let binary: [u32; 4] = [binary[0], binary[1], binary[2], binary[3]];
        Type::V128(unsafe { core::mem::transmute::<[u32; 4], i128>(binary) })
    }
}

fn serialize_types(values: &[Type]) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.extend_from_slice(&(values.len() as u16).to_be_bytes());
    for val in values {
        let tag: u8 = match val {
            Type::Void   => 0,
            Type::I32(_) => 1,
            Type::I64(_) => 2,
            Type::F32(_) => 3,
            Type::F64(_) => 4,
            Type::V128(_) => 5,
        };
        buf.push(tag);

        let encoded = val.encode();

        buf.push(encoded.len() as u8);
        for u in encoded {
            buf.extend_from_slice(&u.to_be_bytes());
        }
    }

    buf
}

fn deserialize_types(data: &[u8]) -> Vec<Type> {
    let mut values = Vec::new();
    if data.len() < 2 {
        return values;
    }

    let count = u16::from_be_bytes([data[0], data[1]]) as usize;
    let mut offset = 2;
    for _ in 0..count {
        if offset >= data.len() {
            break;
        }

        let tag = data[offset];
        offset += 1;
        if offset >= data.len() {
            break;
        }

        let len = data[offset] as usize;
        offset += 1;

        if offset + len * 4 > data.len() {
            break;
        }
        let mut u32_vec = Vec::new();
        for i in 0..len {
            let start = offset + i * 4;
            let u = u32::from_be_bytes(data[start..start+4].try_into().unwrap());
            u32_vec.push(u);
        }
        offset += len * 4;

        let value = match tag {
            0 => Type::Void,
            1 => Type::decode_to_i32(u32_vec),
            2 => Type::decode_to_i64(u32_vec),
            3 => Type::decode_to_f32(u32_vec),
            4 => Type::decode_to_f64(u32_vec),
            5 => Type::decode_to_v128(u32_vec),
            _ => continue,
        };
        values.push(value);
    }

    values
}

#[derive(Debug, Clone)]
pub enum Message {
    ClientReady,
    ServerTask {
        task_id: u64,
        binary: Vec<u8>,
        params: Vec<Type>,
    },
    ClientResult {
        task_id: u64,
        result: Vec<Type>,
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

                payload.extend(serialize_types(params));
                (0x02, payload)
            }
            Message::ClientResult { task_id, result } => {
                let mut payload = Vec::new();
                payload.extend_from_slice(&task_id.to_be_bytes());

                payload.extend(&serialize_types(result));
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
                let params = deserialize_types(&payload[params_start..].to_vec());

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
                let result = deserialize_types(&payload[8..].to_vec());

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
        let binary = b"foo";
        let params = vec![Type::I64(1000), Type::F64(3.14159)];

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
        let result = vec![Type::I32(10), Type::F32(2.718)];

        let message = Message::ClientResult {
            task_id,
            result: result.clone(),
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
