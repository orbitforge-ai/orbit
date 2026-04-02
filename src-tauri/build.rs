use std::collections::BTreeMap;
use std::path::Path;

fn load_env_file(path: &Path, vars: &mut BTreeMap<String, String>) {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return;
    };

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, val)) = line.split_once('=') {
            vars.insert(key.trim().to_string(), val.trim().to_string());
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=.env");
    println!("cargo:rerun-if-changed=../.env");

    let mut vars = BTreeMap::new();

    // Load the repo-root .env first so src-tauri/.env can override it when needed.
    load_env_file(Path::new("../.env"), &mut vars);
    load_env_file(Path::new(".env"), &mut vars);

    for (key, val) in vars {
        println!("cargo:rustc-env={}={}", key, val);
    }

    tauri_build::build()
}
