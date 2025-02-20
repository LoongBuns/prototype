use std::io::{Read, Write};
use std::net::TcpStream;

use protocol::{Message, Type};
use wamr_rust_sdk::{
    function::Function, instance::Instance, module::Module, runtime::Runtime, value::WasmValue,
};

use crate::Error;

enum ModuleState {
    Starting,
    Loading {
        module_name: String,
        module_chunks: Vec<Vec<u8>>,
        module_params: Vec<Type>,
    },
    Execute {
        module_name: String,
        module_binary: Vec<u8>,
        module_params: Vec<Type>,
    },
    Pending {
        module_name: String,
        module_binary: Vec<u8>,
    },
}

pub fn execute_wasm<T: Into<Vec<u8>>>(binary: T, params: Vec<Type>) -> Result<Vec<Type>, Error> {
    let wasm_params = params
        .iter()
        .map(|f| match f {
            Type::Void => WasmValue::Void,
            Type::I32(v) => WasmValue::I32(*v),
            Type::I64(v) => WasmValue::I64(*v),
            Type::F32(v) => WasmValue::F32(*v),
            Type::F64(v) => WasmValue::F64(*v),
            Type::V128(v) => WasmValue::V128(*v),
        })
        .collect();

    let runtime = Runtime::new()?;
    let module = Module::from_vec(&runtime, binary.into(), "container")?;

    let instance = Instance::new(&runtime, &module, 1024 * 64)?;

    let function = Function::find_export_func(&instance, "run")?;

    let wasm_result = function.call(&instance, &wasm_params)?;

    let result = wasm_result
        .iter()
        .map(|f| match f {
            WasmValue::Void => Type::Void,
            WasmValue::I32(v) => Type::I32(*v),
            WasmValue::I64(v) => Type::I64(*v),
            WasmValue::F32(v) => Type::F32(*v),
            WasmValue::F64(v) => Type::F64(*v),
            WasmValue::V128(v) => Type::V128(*v),
        })
        .collect();
    Ok(result)
}

fn handle_connection(mut socket: TcpStream) -> Result<(), Error> {
    let mut module_state = ModuleState::Starting;
    let mut buf = [0u8; 2048];

    let ready_message = Message::ClientReady {
        module_name: None,
        device_ram: 0,
    };
    socket.write_all(&ready_message.encode())?;

    loop {
        let n = socket.read(&mut buf)?;

        match Message::decode(&buf[..n])? {
            Message::ServerTask {
                task_id,
                module,
                params,
            } => match module_state {
                ModuleState::Starting => {
                    module_state = ModuleState::Loading {
                        module_name: module.name,
                        module_chunks: vec![Vec::new(); module.total_chunks as usize],
                        module_params: params,
                    };
                }
                ModuleState::Pending {
                    module_name,
                    module_binary,
                } => {
                    if module.name == module_name {
                        let result = execute_wasm(module_binary, params.clone())?;
                        let result_msg = Message::ClientResult { task_id, result };
                        socket.write_all(&result_msg.encode()?)?;
                        module_state = Module::Execute {
                            module_name,
                            module_binary,
                            module_params: params,
                        }
                    } else {
                        module_state = ModuleState::Loading {
                            module_name: module.name,
                            module_chunks: vec![Vec::new(); module.total_chunks as usize],
                            module_params: params,
                        };
                    }
                }
                _ => {}
            },
            Message::ServerModuleChunk {
                task_id,
                chunk_index,
                chunk_data,
            } => match module_state {
                ModuleState::Loading {
                    module_name,
                    module_chunks,
                    module_params,
                } => {
                    if chunk_index < module_chunks.len() {
                        module_chunks[chunk_index as usize] = chunk_data;
                        if module_chunks.iter().all(|c| !c.is_empty()) {
                            let binary: Vec<u8> = module_chunks.concat();
                            let result = execute_wasm(binary, cache.params.clone())?;
                            let result_msg = Message::ClientResult { task_id, result };
                            socket.write_all(&result_msg.encode()?)?;
                            module_state = Module::Execute {
                                module_name,
                                module_binary: binary,
                                module_params,
                            }
                        }
                    }
                }
                _ => {}
            },
            Message::ServerAck { .. } => match module_state {
                ModuleState::Execute {
                    module_name,
                    module_binary,
                    ..
                } => {
                    let ready_message = Message::ClientReady {
                        module_name: None,
                        device_ram: 0,
                    };
                    socket.write_all(&ready_message.encode())?;
                    module_state = Module::Pending {
                        module_name,
                        module_binary,
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

pub fn setup_container(host: &str, port: u16) -> Result<(), Error> {
    let addr = format!("{}:{}", host, port);

    let stream = TcpStream::connect(&addr)?;

    handle_connection(stream)?;

    Ok(())
}
