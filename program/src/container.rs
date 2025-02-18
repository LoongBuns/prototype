use std::io::{Read, Write};
use std::net::TcpStream;

use protocol::{Message, Type};
use wamr_rust_sdk::{
    function::Function, instance::Instance, module::Module, runtime::Runtime, value::WasmValue
};

use crate::Error;

fn execute_wasm(binary: Vec<u8>, params: Vec<Type>) -> Result<Vec<Type>, Error> {
    let wasm_params = params.iter().map(|f| match f {
        Type::Void => WasmValue::Void,
        Type::I32(v) => WasmValue::I32(*v),
        Type::I64(v) => WasmValue::I64(*v),
        Type::F32(v) => WasmValue::F32(*v),
        Type::F64(v) => WasmValue::F64(*v),
        Type::V128(v) => WasmValue::V128(*v),
    }).collect();

    let runtime = Runtime::new()?;
    let module = Module::from_vec(&runtime, binary, "container")?;

    let instance = Instance::new(&runtime, &module, 1024 * 64)?;

    let function = Function::find_export_func(&instance, "run")?;

    let wasm_result = function.call(&instance, &wasm_params)?;

    let result = wasm_result.iter().map(|f| match f {
        WasmValue::Void => Type::Void,
        WasmValue::I32(v) => Type::I32(*v),
        WasmValue::I64(v) => Type::I64(*v),
        WasmValue::F32(v) => Type::F32(*v),
        WasmValue::F64(v) => Type::F64(*v),
        WasmValue::V128(v) => Type::V128(*v),
    }).collect();
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
