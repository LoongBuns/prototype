include!(concat!(env!("OUT_DIR"), "/generate.rs"));

#[derive(Debug)]
pub struct WasmModule {
    pub name: &'static str,
    pub data: &'static [u8],
}

pub fn get_wasm_modules() -> &'static [WasmModule] {
    WASM_MODULES
}
