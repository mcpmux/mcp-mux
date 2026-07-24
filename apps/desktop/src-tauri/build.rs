use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    let config_path = Path::new("tauri.conf.json");
    if let Ok(contents) = fs::read_to_string(config_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
            if let Some(identifier) = json.get("identifier").and_then(|v| v.as_str()) {
                println!("cargo:rustc-env=TAURI_APP_IDENTIFIER={}", identifier);
            }
        }
    }
    println!("cargo:rerun-if-changed=tauri.conf.json");

    // Stamp git/build metadata into the binary so the admin UI can detect a stale
    // SPA build (web-admin serves a pre-built bundle from `apps/desktop/dist`).
    let git_sha = git_output(&["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let git_branch =
        git_output(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let commit_time =
        git_output(&["log", "-1", "--format=%ci"]).unwrap_or_else(|| "unknown".to_string());
    let build_time = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(format_epoch)
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| format_epoch(d.as_secs()))
                .unwrap_or_else(|_| "unknown".to_string())
        });

    println!("cargo:rustc-env=MCPMUX_BUILD_GIT_SHA={}", git_sha);
    println!("cargo:rustc-env=MCPMUX_BUILD_GIT_BRANCH={}", git_branch);
    println!("cargo:rustc-env=MCPMUX_BUILD_COMMIT_TIME={}", commit_time);
    println!("cargo:rustc-env=MCPMUX_BUILD_TIME={}", build_time);
    println!("cargo:rerun-if-changed=../../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../../.git/logs/HEAD");

    tauri_build::build()
}

fn git_output(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Format a Unix timestamp as a naive UTC datetime string (no external deps).
fn format_epoch(secs: u64) -> String {
    let days = secs / 86400;
    let rem = secs % 86400;
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    let ss = rem % 60;

    let jdn = days + 2440588;
    let a = jdn + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - (146097 * b) / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4800 + m / 10;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        year, month, day, hh, mm, ss
    )
}
