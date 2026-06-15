//! One-click IDE install commands.
//!
//! Opens deep link URIs for VS Code and Cursor to install the McpMux MCP server.

use mcpmux_core::{cursor_deep_link, vscode_deep_link};
use tracing::info;

/// Add McpMux to VS Code via deep link.
#[tauri::command]
pub async fn add_to_vscode(gateway_url: String) -> Result<(), String> {
    let uri = vscode_deep_link(&gateway_url);
    info!("[ClientInstall] Opening VS Code deep link: {}", uri);
    open_deep_link(&uri)
}

/// Add McpMux to Cursor via deep link.
#[tauri::command]
pub async fn add_to_cursor(gateway_url: String) -> Result<(), String> {
    let uri = cursor_deep_link(&gateway_url);
    info!("[ClientInstall] Opening Cursor deep link: {}", uri);
    open_deep_link(&uri)
}

/// Open a deep link URI using the system handler.
fn open_deep_link(uri: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        open_url_shell_execute(uri)
    }
    #[cfg(not(target_os = "windows"))]
    {
        open::that(uri).map_err(|e| format!("Failed to open URI: {}", e))
    }
}

/// Windows: Use ShellExecuteW to open URI without flashing a console window.
#[cfg(target_os = "windows")]
fn open_url_shell_execute(url: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_cmd: i32,
        ) -> isize;
    }

    let url_wide: Vec<u16> = OsStr::new(url).encode_wide().chain(Some(0)).collect();
    let open_wide: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();

    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            open_wide.as_ptr(),
            url_wide.as_ptr(),
            ptr::null(),
            ptr::null(),
            1, // SW_SHOWNORMAL
        )
    };

    if result > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecuteW failed with code: {}", result))
    }
}
