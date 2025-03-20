use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use program::*;
use wamr_rust_sdk::{
    function::Function, instance::Instance, module::Module, runtime::Runtime, value::WasmValue,
    RuntimeError,
};

pub struct SystemClock;

impl Clock for SystemClock {
    fn timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

pub struct WasmExecutor;

impl Executor for WasmExecutor {
    type Error = RuntimeError;

    fn execute(&self, binary: &[u8], params: Vec<Type>) -> Result<Vec<Type>, Self::Error> {
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
        let module = Module::from_vec(&runtime, binary.to_vec(), "container")?;

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
}

pub struct TcpTransport {
    stream: TcpStream,
}

impl TcpTransport {
    pub fn new(addr: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let stream = TcpStream::connect(addr)?;
        stream.set_nonblocking(true)?;
        Ok(Self { stream })
    }
}

impl Transport for TcpTransport {
    type Error = std::io::Error;

    fn read<'a, B>(&mut self, buf: &'a mut B) -> Result<usize, Self::Error>
    where
        B: BufMut + ?Sized,
    {
        let mut buffer = [0u8; 2048];
        let bytes_read = match self.stream.read(&mut buffer) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => 0,
            Err(e) => return Err(e),
        };
        buf.put_slice(&buffer[..bytes_read]);
        Ok(bytes_read)
    }

    fn write<'a, B>(&mut self, src: &'a mut B) -> Result<usize, Self::Error>
    where
        B: Buf + ?Sized,
    {
        let src_bytes = src.chunk();
        let bytes_written = match self.stream.write(src_bytes) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => 0,
            Err(e) => return Err(e),
        };
        Ok(bytes_written)
    }
}

fn main() {
    let Config { host, dispatcher_port, .. } = Config::new();
    let addr = format!("{}:{}", host, dispatcher_port);

    env_logger::init();

    let transport = loop {
        match TcpTransport::new(&addr) {
            Ok(t) => break t,
            Err(e) => {
                log::error!("Connection failed: {}, retrying in 10 seconds...", e);
                std::thread::sleep(Duration::from_secs(10));
            }
        }
    };

    let executor = WasmExecutor;
    let clock = SystemClock;

    let mut session = Session::new(transport, executor, clock, 1024 * 64);

    session.run().unwrap();
}
