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
            "fractal" => {
                const WIDTH: i32 = 800;
                const HEIGHT: i32 = 600;
                const CHUNK_SIZE: i32 = 100;
                const CENTER_X: f64 = 0.0;
                const ZOOM: f64 = 1.0;
                const MAX_ITER: i32 = 50;

                for start_row in (0..HEIGHT).step_by(CHUNK_SIZE as usize) {
                    let end_row = (start_row + CHUNK_SIZE).min(HEIGHT);

                    modules.push(Module {
                        name: "fractal".into(),
                        binary: module.binary.to_vec(),
                        params: vec![
                            Type::I32(WIDTH),
                            Type::I32(HEIGHT),
                            Type::I32(start_row),
                            Type::I32(end_row),
                            Type::F64(CENTER_X),
                            Type::F64(ZOOM),
                            Type::I32(MAX_ITER),
                        ],
                    });
                }
            }
            _ => {}
        }
    }

    modules
}
