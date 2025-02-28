use alloc::vec::Vec;

use protocol::Type;
use wamr_rust_sdk::{
    function::Function, instance::Instance, module::Module, runtime::Runtime, value::WasmValue,
};

use crate::Error;

pub struct Executor;

impl Executor {
    pub fn execute(&mut self, module: Vec<u8>, params: Vec<Type>) -> Result<Vec<Type>, Error> {
        let wasm_params = params
            .iter()
            .map(|v| match v {
                Type::Void => WasmValue::Void,
                Type::I32(v) => WasmValue::I32(*v),
                Type::I64(v) => WasmValue::I64(*v),
                Type::F32(v) => WasmValue::F32(*v),
                Type::F64(v) => WasmValue::F64(*v),
                Type::V128(v) => WasmValue::V128(*v),
            })
            .collect();

        let runtime = Runtime::new()?;
        let module = Module::from_vec(&runtime, module, "container")?;

        let instance = Instance::new(&runtime, &module, 1024 * 64)?;

        let function = Function::find_export_func(&instance, "run")?;
    
        let wasm_result = function.call(&instance, &wasm_params)?;
    
        let result = wasm_result
            .iter()
            .map(|wasm_v| match wasm_v {
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
}