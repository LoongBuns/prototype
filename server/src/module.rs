use protocol::Type;

include!(concat!(env!("OUT_DIR"), "/generate.rs"));

#[derive(Debug)]
pub struct StaticModule {
    pub name: &'static str,
    pub binary: &'static [u8],
}

fn get_static_modules() -> &'static [StaticModule] {
    STATIC_MODULES
}

#[derive(Debug)]
pub struct Module {
    pub name: String,
    pub binary: Vec<u8>,
    pub params: Vec<Type>,
}

pub fn load_modules() -> Vec<Module> {
    let mut modules = Vec::new();

    for module in get_static_modules().iter() {
        match module.name {
            "render" => {
                const HEIGHT: i32 = 300;
                const CHUNK_SIZE: i32 = 100;

                for start_row in (0..HEIGHT).step_by(CHUNK_SIZE as usize) {
                    let end_row = (start_row + CHUNK_SIZE).min(HEIGHT);

                    modules.push(Module {
                        name: "render".into(),
                        binary: module.binary.to_vec(),
                        params: vec![Type::I32(start_row), Type::I32(end_row)],
                    });
                }
            }
            "fiber" => {
                modules.push(Module {
                    name: "fiber".into(),
                    binary: module.binary.to_vec(),
                    params: vec![],
                });
            }
            _ => {}
        }
    }

    modules
}
