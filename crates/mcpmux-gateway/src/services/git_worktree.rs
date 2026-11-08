//! Map a git linked worktree to its main checkout.
//!
//! Workspace bindings match a reported root by exact path, and a linked
//! worktree is a different path from the checkout it was cut from. Agents run
//! in throwaway worktrees (`<repo>/.worktrees/<name>`), so without this they
//! land on no binding and lose every tool bound to the repo.
//!
//! Reads `.git` files only, no `git` process: the resolver calls this on the
//! hot path after an exact miss.

use std::fs;
use std::path::{Path, PathBuf};

use mcpmux_core::normalize_workspace_root;

/// Upper bound on a `.git` pointer file or `gitdir` back-pointer we will read.
const POINTER_FILE_MAX_BYTES: u64 = 4096;

/// The main checkout `root` is a linked worktree of, as a normalized root.
///
/// `None` unless every check passes, so a caller falls back to treating the
/// root as unbound:
/// - `<root>/.git` is a file holding `gitdir: <main>/.git/worktrees/<name>`
/// - that directory exists and has a `gitdir` file pointing back at `<root>/.git`
///
/// The back-pointer is what makes this safe. Anyone can write a `.git` file
/// that names another repo's worktree dir, but only `git worktree add` in that
/// repo writes the matching back-pointer, so a forged file cannot borrow the
/// other repo's bindings (and the tool credentials behind them).
///
/// ponytail: the result is canonical, so a binding stored under a symlinked
/// path will not match. Windows `\\?\` canonical paths also will not match.
/// Both fall back to unbound, as before.
pub fn main_checkout_root(root: &str) -> Option<String> {
    let root = Path::new(root);
    if !root.is_absolute() {
        return None;
    }
    let dot_git = root.join(".git");
    if !dot_git.is_file() {
        return None;
    }

    let admin_dir = admin_dir_of(root, &read_small(&dot_git)?)?;
    let admin_dir = fs::canonicalize(admin_dir).ok()?;

    let worktrees_dir = admin_dir.parent()?;
    let main_git_dir = worktrees_dir.parent()?;
    if worktrees_dir.file_name()? != "worktrees" || main_git_dir.file_name()? != ".git" {
        return None;
    }

    // Relative back-pointers (git 2.48+ with worktree.useRelativePaths) are
    // relative to the admin dir, same rule as `.git` pointer targets in
    // `admin_dir_of`. PathBuf::join leaves an absolute path unchanged.
    let back_pointer = admin_dir.join(read_small(&admin_dir.join("gitdir"))?.trim());
    if fs::canonicalize(back_pointer).ok()? != fs::canonicalize(&dot_git).ok()? {
        return None;
    }

    let main_root = main_git_dir.parent()?;
    Some(normalize_workspace_root(&main_root.to_string_lossy()))
}

/// The `gitdir:` target of a `.git` pointer file, relative targets resolved
/// against the worktree root.
fn admin_dir_of(root: &Path, pointer: &str) -> Option<PathBuf> {
    let target = pointer
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))?
        .trim();
    if target.is_empty() {
        return None;
    }
    Some(root.join(target))
}

fn read_small(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > POINTER_FILE_MAX_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A main checkout with one linked worktree, laid out as `git worktree add`
    /// leaves it. Returns (tempdir, main root, worktree root).
    fn linked_worktree() -> (TempDir, PathBuf, PathBuf) {
        let temp = TempDir::new().unwrap();
        let base = fs::canonicalize(temp.path()).unwrap();
        let main = base.join("main");
        let worktree = main.join(".worktrees").join("wt1");
        let admin = main.join(".git").join("worktrees").join("wt1");
        fs::create_dir_all(&admin).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .unwrap();
        fs::write(
            admin.join("gitdir"),
            format!("{}\n", worktree.join(".git").display()),
        )
        .unwrap();
        (temp, main, worktree)
    }

    fn root_str(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn worktree_maps_to_its_main_checkout() {
        let (_temp, main, worktree) = linked_worktree();
        assert_eq!(
            main_checkout_root(&root_str(&worktree)),
            Some(normalize_workspace_root(&root_str(&main)))
        );
    }

    #[test]
    fn relative_gitdir_is_resolved_against_the_worktree() {
        let (_temp, main, worktree) = linked_worktree();
        fs::write(worktree.join(".git"), "gitdir: ../../.git/worktrees/wt1\n").unwrap();
        assert_eq!(
            main_checkout_root(&root_str(&worktree)),
            Some(normalize_workspace_root(&root_str(&main)))
        );
    }

    #[test]
    fn relative_back_pointer_is_resolved_against_the_admin_dir() {
        let (_temp, main, worktree) = linked_worktree();
        let admin = main.join(".git").join("worktrees").join("wt1");
        // git 2.48+ with worktree.useRelativePaths writes a path relative to
        // the admin dir's parent of the back-pointer file itself.
        fs::write(admin.join("gitdir"), "../../../.worktrees/wt1/.git\n").unwrap();
        assert_eq!(
            main_checkout_root(&root_str(&worktree)),
            Some(normalize_workspace_root(&root_str(&main)))
        );
    }

    #[test]
    fn main_checkout_and_plain_folder_do_not_map() {
        let (temp, main, _worktree) = linked_worktree();
        assert_eq!(main_checkout_root(&root_str(&main)), None);
        assert_eq!(main_checkout_root(&root_str(temp.path())), None);
        assert_eq!(main_checkout_root("relative/path"), None);
    }

    #[test]
    fn forged_git_file_without_back_pointer_does_not_map() {
        let (temp, main, _worktree) = linked_worktree();
        let intruder = temp.path().join("intruder");
        fs::create_dir_all(&intruder).unwrap();
        fs::write(
            intruder.join(".git"),
            format!(
                "gitdir: {}\n",
                main.join(".git").join("worktrees").join("wt1").display()
            ),
        )
        .unwrap();
        assert_eq!(main_checkout_root(&root_str(&intruder)), None);
    }

    #[test]
    fn gitdir_outside_a_worktrees_dir_does_not_map() {
        let (temp, _main, worktree) = linked_worktree();
        let elsewhere = temp.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(
            elsewhere.join("gitdir"),
            format!("{}\n", worktree.join(".git").display()),
        )
        .unwrap();
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", elsewhere.display()),
        )
        .unwrap();
        assert_eq!(main_checkout_root(&root_str(&worktree)), None);
    }

    #[test]
    fn missing_admin_dir_or_garbage_pointer_does_not_map() {
        let (_temp, _main, worktree) = linked_worktree();
        fs::write(worktree.join(".git"), "not a pointer\n").unwrap();
        assert_eq!(main_checkout_root(&root_str(&worktree)), None);
        fs::write(worktree.join(".git"), "gitdir: /no/such/dir\n").unwrap();
        assert_eq!(main_checkout_root(&root_str(&worktree)), None);
    }
}
