//! Owner-only directories for the data directory and logs.

use std::path::Path;

/// Create `dir` (with any missing parents) and make it private to the
/// current user.
///
/// On Unix the directory must be owned by the effective user, and it is
/// chmod'ed to `0700` when group or others have any access. Everything
/// kept inside (the database and its WAL, Space configs, logs, key files)
/// is then out of other local users' reach whatever mode the files
/// themselves were created with, so the process umask doesn't have to be
/// changed (which would also change it for the MCP servers it spawns).
///
/// On other platforms the per-user profile ACLs already apply; the
/// directory is only created.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let meta = std::fs::metadata(dir)?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        let euid = unsafe { libc::geteuid() };
        if meta.uid() != euid {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "{} is owned by another user (uid {}); refusing to keep data there",
                    dir.display(),
                    meta.uid()
                ),
            ));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)?;
    Ok(())
}

/// Restrict an existing file to its owner (`0600`) on Unix; no-op elsewhere
/// or when the file doesn't exist.
pub fn restrict_file(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(meta) if meta.permissions().mode() & 0o077 != 0 => {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn creates_new_directories_owner_only() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/b/data");
        ensure_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn tightens_an_existing_open_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("data");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o775)).unwrap();
        ensure_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn restricts_files_to_their_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("mcpmux.db");
        std::fs::write(&file, b"x").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        restrict_file(&file).unwrap();
        assert_eq!(mode(&file), 0o600);
        restrict_file(&tmp.path().join("missing")).unwrap();
    }
}
