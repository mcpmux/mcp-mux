use std::fs;
use std::path::Path;

fn main() {
    // Read tauri.conf.json to extract the app identifier
    // This ensures a single source of truth for the identifier
    let config_path = Path::new("tauri.conf.json");
    if let Ok(contents) = fs::read_to_string(config_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
            if let Some(identifier) = json.get("identifier").and_then(|v| v.as_str()) {
                println!("cargo:rustc-env=TAURI_APP_IDENTIFIER={}", identifier);
            }
        }
    }
    
    // Tell Cargo to re-run this script if tauri.conf.json changes
    println!("cargo:rerun-if-changed=tauri.conf.json");
    
    tauri_build::build()
}
