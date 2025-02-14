use std::io::{Read, Write};
use std::net::TcpStream;

use protocol::Message;
use wamr_rust_sdk::{
    function::Function, instance::Instance, module::Module, runtime::Runtime, value::WasmValue
};

use crate::Error;

fn serialize_wasm_values(values: &[WasmValue]) -> Vec<u8> {
    let mut buf = Vec::new();

    buf.extend_from_slice(&(values.len() as u16).to_be_bytes());
    for val in values {
        let tag: u8 = match val {
            WasmValue::Void   => 0,
            WasmValue::I32(_) => 1,
            WasmValue::I64(_) => 2,
            WasmValue::F32(_) => 3,
            WasmValue::F64(_) => 4,
            WasmValue::V128(_) => 5,
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

fn deserialize_wasm_values(data: &[u8]) -> Vec<WasmValue> {
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
            0 => WasmValue::Void,
            1 => WasmValue::decode_to_i32(u32_vec),
            2 => WasmValue::decode_to_i64(u32_vec),
            3 => WasmValue::decode_to_f32(u32_vec),
            4 => WasmValue::decode_to_f64(u32_vec),
            5 => WasmValue::decode_to_v128(u32_vec),
            _ => continue,
        };
        values.push(value);
    }
    values
}

fn execute_wasm(binary: Vec<u8>, params: Vec<u8>) -> Result<Vec<u8>, Error> {
    let wasm_params = deserialize_wasm_values(&params);

    let runtime = Runtime::new()?;
    let module = Module::from_vec(&runtime, binary, "container")?;

    let instance = Instance::new(&runtime, &module, 1024 * 64)?;

    let function = Function::find_export_func(&instance, "run")?;

    let wasm_result = function.call(&instance, &wasm_params)?;

    let result = serialize_wasm_values(&wasm_result);
    Ok(result)
}

fn handle_response(mut socket: TcpStream) -> Result<(), Error> {
    socket.write_all(&Message::ClientReady.serialize())?;
    let mut buf = [0u8; 1024];

    loop {
        let n = socket.read(&mut buf)?;

        match Message::deserialize(&buf[..n]) {
            Ok(Message::ServerTask { task_id, binary, params }) => {
                let result = execute_wasm(binary, params)?;
                
                let msg = Message::ClientResult {
                    task_id,
                    result,
                };
                socket.write_all(&msg.serialize())?;
            }
            Ok(Message::ServerAck { .. }) => {
                socket.write_all(&Message::ClientReady.serialize())?;
            }
            _ => {}
        }
    }
}

pub fn setup_container(host: &str, port: u16) -> Result<(), Error> {
    let addr = format!("{}:{}", host, port);

    let stream = TcpStream::connect(&addr)?;

    handle_response(stream)?;

    Ok(())
}
