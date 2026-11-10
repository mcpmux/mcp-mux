//! Default data-directory resolution.
//!
//! Resolution order, in priority:
//! 1. `--data-dir` flag value (handled by callers, validated here).
//! 2. `MCPMUX_DATA_DIR` environment variable (operator override).
//! 3. `$XDG_STATE_HOME/mcpmux` (Linux/macOS standard).
//! 4. `~/.local/state/mcpmux` (XDG default when `XDG_STATE_HOME` is unset).
//! 5. Platform fallback (`~/Library/Application Support/mcpmux` on macOS,
//!    `%LOCALAPPDATA%/mcpmux` on Windows, `~/.local/state/mcpmux` elsewhere).

use std::path::{Path, PathBuf};

use crate::error::RuntimeError;

/// Directory name under the XDG / platform data root.
pub const DATA_DIR_NAME: &str = "mcpmux";

/// Resolve the default data directory using the XDG / platform conventions.
///
/// Does NOT touch the filesystem. Callers pass the result to
/// `RuntimeBuilder::with_data_dir` (which validates and creates the
/// directory) or use it directly to discover an existing install.
pub fn default_data_dir() -> PathBuf {
    if let Some(env) = std::env::var_os("MCPMUX_DATA_DIR") {
        if !env.is_empty() {
            return PathBuf::from(env);
        }
    }

    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        if !state_home.is_empty() {
            return PathBuf::from(state_home).join(DATA_DIR_NAME);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            return home.join("Library/Application Support").join(DATA_DIR_NAME);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(local) = dirs::data_local_dir() {
            return local.join(DATA_DIR_NAME);
        }
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state")
        .join(DATA_DIR_NAME)
}

/// Resolve and validate the data directory for the runtime.
///
/// `override_path` (typically the `--data-dir` flag) wins when `Some`.
/// Otherwise the XDG default is used. A relative path is joined onto the
/// current directory; an absolute path is returned verbatim.
///
/// The path is deliberately NOT canonicalized. Stored rows embed it (e.g.
/// `user_config:<data_dir>/spaces/<id>.json` installation sources) and are
/// matched by exact string, so it must stay identical to what earlier
/// versions wrote. Canonicalizing would also yield a `\\?\C:\...` verbatim
/// path on Windows and differ between the first run (directory missing)
/// and later runs. The data-dir lock does not need it: the kernel lock is
/// per file, whatever spelling of the path opened it.
pub fn resolve_data_dir(override_path: Option<&Path>) -> Result<PathBuf, RuntimeError> {
    let raw = match override_path {
        Some(p) => p.to_path_buf(),
        None => default_data_dir(),
    };

    if raw.as_os_str().is_empty() {
        return Err(RuntimeError::InvalidDataDir {
            path: raw,
            reason: "empty path".to_string(),
        });
    }

    if raw.is_absolute() {
        return Ok(raw);
    }

    Ok(std::env::current_dir()
        .map_err(|e| RuntimeError::InvalidDataDir {
            path: raw.clone(),
            reason: format!("could not resolve relative path: {}", e),
        })?
        .join(&raw))
}

/// Directory holding the daemon's control socket:
/// `$XDG_RUNTIME_DIR/mcpmux/<data-dir-id>`.
///
/// `XDG_RUNTIME_DIR` is the correct home for a user-scoped, non-persistent
/// socket — it is created by systemd-logind with `0700` permissions and is
/// cleared on logout. `<data-dir-id>` is a stable hash of the data directory,
/// so daemons with different `--data-dir` values get separate sockets and a
/// CLI given `--data-dir X` reaches the daemon that owns `X`. When the
/// variable is unset (possible in a bare SSH session), fall back to
/// `<data_dir>/run`, which is per data directory already.
pub fn control_dir(data_dir: &Path) -> PathBuf {
    control_dir_under(
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        data_dir,
    )
}

/// [`control_dir`] with an explicit runtime directory instead of reading
/// `XDG_RUNTIME_DIR` (`None` or empty selects the `<data_dir>/run` fallback).
pub fn control_dir_under(runtime_dir: Option<PathBuf>, data_dir: &Path) -> PathBuf {
    match runtime_dir {
        Some(rt) if !rt.as_os_str().is_empty() => rt
            .join(DATA_DIR_NAME)
            .join(format!("{:016x}", data_dir_id(data_dir))),
        _ => data_dir.join("run"),
    }
}

/// Path to the daemon control socket (Unix) or Named Pipe (Windows).
///
/// On Unix, returns `<control_dir>/control.sock`.
/// On Windows, returns `\\.\pipe\mcpmux-<data-dir-id>`.
pub fn control_socket_path(data_dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        control_dir(data_dir).join("control.sock")
    }
    #[cfg(windows)]
    {
        control_pipe_path(data_dir)
    }
    #[cfg(not(any(unix, windows)))]
    {
        control_dir(data_dir).join("control.sock")
    }
}

/// Windows named pipe path for the daemon: `\\.\pipe\mcpmux-<data-dir-id>`.
pub fn control_pipe_path(data_dir: &Path) -> PathBuf {
    PathBuf::from(format!(r"\\.\pipe\mcpmux-{:016x}", data_dir_id(data_dir)))
}

/// Stable 64-bit FNV-1a hash of the data directory. Spelled out rather than
/// `DefaultHasher` so a CLI and a daemon from different builds agree. The
/// path is normalized first so `/x/` and `/x` map to the same socket.
fn data_dir_id(data_dir: &Path) -> u64 {
    let normalized: PathBuf = data_dir.components().collect();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in normalized.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn environment_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn restore_environment_variable(key: &str, value: Option<std::ffi::OsString>) {
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    #[test]
    fn default_is_under_xdg_state_home_or_home() {
        let _environment = environment_lock();
        let dir = default_data_dir();
        assert!(dir.ends_with(DATA_DIR_NAME));
    }

    #[test]
    fn xdg_state_home_takes_precedence() {
        let _environment = environment_lock();
        let previous = std::env::var_os("XDG_STATE_HOME");
        unsafe {
            std::env::set_var("XDG_STATE_HOME", "/custom/state");
        }
        let dir = default_data_dir();
        assert_eq!(dir, PathBuf::from("/custom/state").join(DATA_DIR_NAME));
        restore_environment_variable("XDG_STATE_HOME", previous);
    }

    #[test]
    fn env_override_wins_over_xdg() {
        let _environment = environment_lock();
        let previous_data_dir = std::env::var_os("MCPMUX_DATA_DIR");
        let previous_state_home = std::env::var_os("XDG_STATE_HOME");
        unsafe {
            std::env::set_var("MCPMUX_DATA_DIR", "/explicit/override");
            std::env::set_var("XDG_STATE_HOME", "/should/be/ignored");
        }
        let dir = default_data_dir();
        assert_eq!(dir, PathBuf::from("/explicit/override"));
        restore_environment_variable("MCPMUX_DATA_DIR", previous_data_dir);
        restore_environment_variable("XDG_STATE_HOME", previous_state_home);
    }

    #[test]
    fn empty_override_path_is_rejected() {
        let _environment = environment_lock();
        let err = resolve_data_dir(Some(Path::new(""))).unwrap_err();
        assert!(matches!(err, RuntimeError::InvalidDataDir { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn absolute_data_dir_is_kept_verbatim_not_canonicalized() {
        let _environment = environment_lock();
        let target = tempfile::tempdir().unwrap();
        let links = tempfile::tempdir().unwrap();
        let link = links.path().join("data-link");
        std::os::unix::fs::symlink(target.path(), &link).unwrap();

        // Existing directory reached through a symlink: still the same string.
        assert_eq!(resolve_data_dir(Some(&link)).unwrap(), link);
        // Missing directory: same rule, so first and later runs agree.
        let missing = links.path().join("not-created-yet");
        assert_eq!(resolve_data_dir(Some(&missing)).unwrap(), missing);
    }

    #[cfg(unix)]
    #[test]
    fn control_socket_prefers_xdg_runtime_dir() {
        let _environment = environment_lock();
        let previous = std::env::var_os("XDG_RUNTIME_DIR");
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        }
        let data_dir = Path::new("/var/lib/mcpmux");
        let socket = control_socket_path(data_dir);
        assert!(socket.starts_with("/run/user/1000/mcpmux/"), "{socket:?}");
        assert!(socket.ends_with("control.sock"));
        restore_environment_variable("XDG_RUNTIME_DIR", previous);
    }

    // Unix only, like the control socket itself: the pinned hash is of the
    // `/`-separated path, and Windows re-joins normalized components with `\`.
    #[cfg(unix)]
    #[test]
    fn control_socket_is_per_data_dir_and_stable() {
        let rt = Some(PathBuf::from("/run/user/1000"));
        let a = control_dir_under(rt.clone(), Path::new("/srv/a"));
        let b = control_dir_under(rt.clone(), Path::new("/srv/b"));
        assert_ne!(a, b, "different data dirs must not share a socket");
        assert_eq!(a, control_dir_under(rt.clone(), Path::new("/srv/a/")));
        // Pinned value (FNV-1a of "/srv/a"): a CLI and a daemon from
        // different builds must agree, so the hash must never change.
        assert_eq!(a, PathBuf::from("/run/user/1000/mcpmux/39aff565700eaeb7"));
    }

    #[cfg(unix)]
    #[test]
    fn control_socket_falls_back_under_data_dir() {
        let _environment = environment_lock();
        let previous = std::env::var_os("XDG_RUNTIME_DIR");
        unsafe {
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
        let data_dir = Path::new("/var/lib/mcpmux");
        assert_eq!(
            control_socket_path(data_dir),
            PathBuf::from("/var/lib/mcpmux/run/control.sock")
        );
        restore_environment_variable("XDG_RUNTIME_DIR", previous);
    }

    #[cfg(windows)]
    #[test]
    fn control_pipe_path_format_and_stability() {
        let a = control_pipe_path(Path::new(r"C:\mcpmux\data"));
        let b = control_pipe_path(Path::new(r"C:\mcpmux\data\"));
        assert_eq!(
            a, b,
            "trailing slashes must normalize to the same pipe name"
        );
        let path_str = a.to_str().unwrap();
        assert!(
            path_str.starts_with(r"\\.\pipe\mcpmux-"),
            "must be a valid named pipe path: {path_str}"
        );
        assert_eq!(control_socket_path(Path::new(r"C:\mcpmux\data")), a);
    }
}
