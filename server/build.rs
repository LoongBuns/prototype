use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

fn main() {
    let current_dir = std::env::current_dir().unwrap();
    let workspace_dir = current_dir.parent().unwrap();
    let task_dir = workspace_dir.join("task");
    let src_dir = task_dir.join("src");
    let dist_dir = task_dir.join("dist");

    println!("cargo:rerun-if-changed={}", src_dir.display());

    let output_install = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .current_dir(task_dir.to_str().unwrap())
            .args(["/C", "npm install"])
            .output()
            .unwrap()
    } else {
        Command::new("sh")
            .current_dir(task_dir.to_str().unwrap())
            .args(["-c", "npm install"])
            .output()
            .unwrap()
    };

    if !output_install.status.success() {
        panic!("npm install fail");
    }

    let output_build = if cfg!(target_os = "windows") {
        Command::new("cmd")
            .current_dir(task_dir.to_str().unwrap())
            .args(["/C", "npm run build"])
            .output()
            .unwrap()
    } else {
        Command::new("sh")
            .current_dir(task_dir.to_str().unwrap())
            .args(["-c", "npm run build"])
            .output()
            .unwrap()
    };

    if !output_build.status.success() {
        panic!("npm run build fail");
    }

    let mut generated_code = String::new();
    generated_code.push_str("static STATIC_MODULES: &'static [StaticModule] = &[\n");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("generate.rs");

    for entry in dist_dir.read_dir().unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_lowercase();

        if ext == "wasm" {
            let module_name = path.file_stem().unwrap().to_string_lossy();
            let wasm_bytes = fs::read(&path).unwrap();

            generated_code.push_str(&format!(
                "    StaticModule {{ name: \"{}\", binary: &[ \n",
                module_name
            ));

            let indent = "        ";
            for (i, byte) in wasm_bytes.iter().enumerate() {
                if (i) % 12 == 0 {
                    generated_code.push_str(indent);
                }
                generated_code.push_str(&format!("{}, ", byte));
                if (i + 1) % 12 == 0 {
                    generated_code.push_str("\n");
                }
            }
            generated_code.push_str("    ] },\n");
        }
    }

    generated_code.push_str("];\n");

    let mut file = fs::File::create(&dest_path).unwrap();
    file.write_all(generated_code.as_bytes()).unwrap();
}
