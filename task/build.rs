use std::error::Error;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Project<'a> {
    name: &'a str,
    src: &'a Path,
    dist: &'a Path,
}

fn run_command(cwd: &Path, command: &str) -> Result<(), Box<dyn Error>> {
    let Output { status, stdout, stderr } = if cfg!(target_os = "windows") {
         Command::new("cmd")
            .current_dir(cwd)
            .args(["/C", command])
            .output()?
    } else {
        Command::new("sh")
            .current_dir(cwd)
            .args(["-c", command])
            .output()?
    };

    if !status.success() {
        let err_msg = format!(
            "Command '{}' failed\nSTDOUT: {}\nSTDERR: {}",
            command,
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        return Err(err_msg.into());
    }

    Ok(())
}

fn build_project(cwd: &Path, project: &Project) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={}", project.src.display());

    let mode = if std::env::var("PROFILE")? == "release" {
        "release"
    } else {
        "debug"
    };

    let build_cmd = format!("npm run build --workspace={}", project.name);
    run_command(cwd, &build_cmd)?;

    let source_dir = project.dist.join(mode);
    let dist_dir = cwd.join("dist");

    for entry in source_dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
            let dest = dist_dir.join(path.file_name().unwrap());
            fs::copy(&path, dest)?;
        }
    }

    Ok(())
}

fn generate_static_modules(dist_dir: &Path) -> Result<(), Box<dyn Error>> {
    let out_dir = std::env::var("OUT_DIR")?;
    let dest_path = Path::new(&out_dir).join("generate.rs");
    let mut file = File::create(&dest_path)?;

    writeln!(file, "static STATIC_MODULES: &[StaticModule] = &[")?;

    for entry in dist_dir.read_dir()? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) == Some("wasm") {
            let module_name = path.file_stem()
                .and_then(|n| n.to_str())
                .unwrap();

            let wasm_bytes = fs::read(&path)?;

            writeln!(file, "    StaticModule {{")?;
            writeln!(file, "        name: \"{}\",", module_name)?;
            writeln!(file, "        binary: &[")?;

            for chunk in wasm_bytes.chunks(12) {
                write!(file, "            ")?;
                for byte in chunk {
                    write!(file, "0x{:02x}, ", byte)?;
                }
                writeln!(file)?;
            }

            writeln!(file, "        ],")?;
            writeln!(file, "    }},")?;
        }
    }

    writeln!(file, "];")?;

    Ok(())
}

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist_dir = manifest_dir.join("dist");
    fs::create_dir_all(&dist_dir).unwrap();

    run_command(&manifest_dir, "npm install").unwrap();

    let projects = &[
        Project {
            name: "assembly",
            src: &manifest_dir.join("assembly/src"),
            dist: &manifest_dir.join("assembly/dist"),
        }
    ];
    for project in projects {
        build_project(&manifest_dir, project).unwrap();
    }

    generate_static_modules(&dist_dir).unwrap();
}
