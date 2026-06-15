//! Shell environment resolution for GUI applications
//!
//! On macOS (and Linux), GUI applications launched from Finder/Dock/Spotlight
//! inherit a minimal PATH that typically only includes `/usr/bin:/bin:/usr/sbin:/sbin`.
//! This means tools installed via Homebrew (`/opt/homebrew/bin`), nvm, Volta, fnm,
//! or standard `/usr/local/bin` are invisible to the app.
//!
//! This module resolves the user's full login shell PATH by spawning their default
//! shell with login flags and reading back `$PATH`. The result is cached for the
//! lifetime of the process.

use std::ffi::OsString;
use std::sync::OnceLock;
#[cfg(unix)]
use tracing::{debug, info, warn};

/// Cached shell PATH, resolved once on first access.
static SHELL_PATH: OnceLock<Option<OsString>> = OnceLock::new();

/// Get the user's full shell PATH.
///
/// On Unix (macOS / Linux), this spawns the user's login shell to read the
/// fully-initialized `$PATH`, including entries added by `.zshrc`, `.bashrc`,
/// `.profile`, nvm, Volta, Homebrew, etc.
///
/// On Windows, this returns `None` because Windows GUI apps inherit the full
/// system + user PATH from the registry (no shell sourcing needed).
///
/// The result is cached after the first call.
pub fn get_shell_path() -> Option<&'static OsString> {
    SHELL_PATH
        .get_or_init(|| {
            #[cfg(unix)]
            {
                resolve_unix_shell_path()
            }
            #[cfg(not(unix))]
            {
                None
            }
        })
        .as_ref()
}

/// Resolve the full PATH from the user's login shell on Unix.
///
/// Strategy:
/// 1. Read `$SHELL` to find the user's default shell (falls back to `/bin/sh`)
/// 2. Spawn `$SHELL -l -i -c 'printf "%s" "$PATH"'` to get the fully-initialized PATH
///    - `-l` (login): sources `/etc/profile`, `~/.zprofile` / `~/.bash_profile`
///    - `-i` (interactive): sources `~/.zshrc` / `~/.bashrc` (where nvm/Volta/fnm init lives)
///    - `printf` avoids trailing newlines that `echo` might add
/// 3. If `-i` fails (some shells reject it in non-terminal contexts), retry with just `-l`
/// 4. Merge the resolved PATH with the current process PATH to avoid losing any entries
#[cfg(unix)]
fn resolve_unix_shell_path() -> Option<OsString> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    info!("[ShellEnv] Resolving PATH from login shell: {}", shell);

    // Try interactive login shell first (gets nvm/Volta/fnm paths from .zshrc/.bashrc)
    let shell_path = try_resolve_path_from_shell(&shell, &["-l", "-i", "-c"]).or_else(|| {
        debug!("[ShellEnv] Interactive shell failed, trying login-only");
        try_resolve_path_from_shell(&shell, &["-l", "-c"])
    });

    let shell_path = match shell_path {
        Some(p) if !p.is_empty() => p,
        _ => {
            warn!("[ShellEnv] Could not resolve PATH from shell, using process PATH");
            return None;
        }
    };

    // Merge: shell PATH + current process PATH (to keep any paths the app already has)
    let current_path = std::env::var("PATH").unwrap_or_default();
    let merged = merge_paths(&shell_path, &current_path);

    info!(
        "[ShellEnv] Resolved PATH ({} entries, shell had {} entries)",
        merged.split(':').count(),
        shell_path.split(':').count()
    );
    debug!("[ShellEnv] PATH = {}", merged);

    Some(OsString::from(merged))
}

/// Try to resolve PATH by running the user's shell with the given flags.
///
/// Uses `printf "%s" "$PATH"` instead of `echo $PATH` to avoid:
/// - Trailing newlines from echo
/// - Shell-specific echo behavior differences
#[cfg(unix)]
fn try_resolve_path_from_shell(shell: &str, flags: &[&str]) -> Option<String> {
    use std::process::{Command, Stdio};

    // Build command: $SHELL <flags> 'printf "%s" "$PATH"'
    let mut cmd = Command::new(shell);
    for flag in flags {
        cmd.arg(flag);
    }
    cmd.arg(r#"printf "%s" "$PATH""#);

    // Prevent the child from inheriting stdin (avoids tty issues)
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null()); // Suppress shell startup warnings

    match cmd.output() {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                debug!("[ShellEnv] Shell returned empty PATH");
                None
            } else {
                Some(path)
            }
        }
        Ok(output) => {
            debug!(
                "[ShellEnv] Shell exited with status {} (flags: {:?})",
                output.status, flags
            );
            None
        }
        Err(e) => {
            debug!("[ShellEnv] Failed to spawn shell '{}': {}", shell, e);
            None
        }
    }
}

/// Merge two PATH strings, preserving order and deduplicating.
///
/// The `primary` PATH takes precedence (its entries appear first).
/// Entries from `secondary` are appended only if not already present.
#[cfg(unix)]
fn merge_paths(primary: &str, secondary: &str) -> String {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut merged = Vec::new();

    for entry in primary.split(':').chain(secondary.split(':')) {
        if !entry.is_empty() && seen.insert(entry.to_string()) {
            merged.push(entry.to_string());
        }
    }

    merged.join(":")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // ── merge_paths tests ──────────────────────────────────────────

    #[test]
    fn test_merge_paths_deduplicates() {
        let result = merge_paths("/usr/bin:/usr/local/bin", "/usr/bin:/opt/homebrew/bin");
        assert_eq!(result, "/usr/bin:/usr/local/bin:/opt/homebrew/bin");
    }

    #[cfg(unix)]
    #[test]
    fn test_merge_paths_primary_order_preserved() {
        let result = merge_paths("/a:/b:/c", "/d:/b:/e");
        assert_eq!(result, "/a:/b:/c:/d:/e");
    }

    #[cfg(unix)]
    #[test]
    fn test_merge_paths_empty_entries_skipped() {
        let result = merge_paths("/a::/b", ":/c:");
        assert_eq!(result, "/a:/b:/c");
    }

    #[cfg(unix)]
    #[test]
    fn test_merge_paths_empty_secondary() {
        let result = merge_paths("/a:/b", "");
        assert_eq!(result, "/a:/b");
    }

    #[cfg(unix)]
    #[test]
    fn test_merge_paths_empty_primary() {
        let result = merge_paths("", "/a:/b");
        assert_eq!(result, "/a:/b");
    }

    #[cfg(unix)]
    #[test]
    fn test_merge_paths_both_empty() {
        let result = merge_paths("", "");
        assert_eq!(result, "");
    }

    #[cfg(unix)]
    #[test]
    fn test_merge_paths_identical() {
        let result = merge_paths("/a:/b:/c", "/a:/b:/c");
        assert_eq!(result, "/a:/b:/c");
    }

    #[cfg(unix)]
    #[test]
    fn test_merge_paths_many_duplicates() {
        let result = merge_paths("/a:/b:/c:/d", "/d:/c:/b:/a:/e");
        assert_eq!(result, "/a:/b:/c:/d:/e");
    }

    // ── get_shell_path tests ───────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn test_get_shell_path_returns_something() {
        // On any Unix system with a shell, this should succeed
        let path = get_shell_path();
        assert!(path.is_some(), "Should resolve shell PATH on Unix");
        let path_str = path.unwrap().to_string_lossy();
        assert!(
            path_str.contains("/usr/bin") || path_str.contains("/bin"),
            "PATH should contain standard directories: {}",
            path_str
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_get_shell_path_is_cached() {
        // Calling twice should return the exact same reference (OnceLock)
        let first = get_shell_path();
        let second = get_shell_path();
        assert!(first.is_some());
        assert!(second.is_some());
        // Same pointer — verifies caching via OnceLock
        assert!(std::ptr::eq(first.unwrap(), second.unwrap()));
    }

    #[cfg(unix)]
    #[test]
    fn test_get_shell_path_has_no_trailing_newline() {
        let path = get_shell_path();
        if let Some(p) = path {
            let s = p.to_string_lossy();
            assert!(
                !s.ends_with('\n') && !s.ends_with('\r'),
                "PATH should not have trailing newlines: {:?}",
                s
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_get_shell_path_entries_are_not_empty() {
        let path = get_shell_path();
        if let Some(p) = path {
            let s = p.to_string_lossy();
            for entry in s.split(':') {
                assert!(
                    !entry.is_empty(),
                    "PATH should not contain empty entries: {:?}",
                    s
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_get_shell_path_is_valid_utf8() {
        let path = get_shell_path();
        if let Some(p) = path {
            assert!(p.to_str().is_some(), "PATH should be valid UTF-8: {:?}", p);
        }
    }

    // ── try_resolve_path_from_shell tests ──────────────────────────

    #[cfg(unix)]
    #[test]
    fn test_try_resolve_shell_with_login_flag() {
        // /bin/sh should work with -l -c
        let result = try_resolve_path_from_shell("/bin/sh", &["-l", "-c"]);
        assert!(result.is_some(), "Should resolve PATH from /bin/sh -l -c");
        let path = result.unwrap();
        assert!(!path.is_empty(), "PATH should not be empty");
        assert!(
            path.contains("/bin") || path.contains("/usr"),
            "PATH should contain standard dirs: {}",
            path
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_try_resolve_shell_nonexistent_shell() {
        let result = try_resolve_path_from_shell("/nonexistent/shell_binary_xyz", &["-l", "-c"]);
        assert!(result.is_none(), "Should fail for nonexistent shell");
    }

    #[cfg(unix)]
    #[test]
    fn test_try_resolve_shell_invalid_flags() {
        // --bogus-flag should cause the shell to error
        let result = try_resolve_path_from_shell("/bin/sh", &["--bogus-flag-xyz", "-c"]);
        assert!(result.is_none(), "Should fail with invalid shell flags");
    }
}
