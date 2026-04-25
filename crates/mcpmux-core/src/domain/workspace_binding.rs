//! WorkspaceBinding entity — maps a workspace root on disk to a concrete
//! (Space, FeatureSet) pair.
//!
//! Bindings are the only override surface for FS resolution:
//!
//!   workspace root matches a binding?  →  (binding.space_id, binding.feature_set_id)
//!                                  else  →  (default Space, its seeded Default FS)
//!
//! Path handling is **platform-agnostic**. A binding written on Windows
//! (`d:\work\proj`) has to match correctly on a Linux host that's just
//! reading the DB (and vice versa). We detect the path style from the
//! string itself — drive-letter prefix ⇒ Windows, leading `/` ⇒ POSIX —
//! rather than from `cfg!(windows)`. Both separators are accepted for
//! prefix matching regardless of the host OS.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A binding between a normalized workspace root and a concrete
/// (Space, FeatureSet) pair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceBinding {
    pub id: Uuid,
    pub workspace_root: String,
    pub space_id: Uuid,
    pub feature_set_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkspaceBinding {
    pub fn new(
        workspace_root: impl Into<String>,
        space_id: Uuid,
        feature_set_id: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            workspace_root: workspace_root.into(),
            space_id,
            feature_set_id: feature_set_id.into(),
            created_at: now,
            updated_at: now,
        }
    }
}

// ============================================================================
// Path style detection
// ============================================================================

/// Which family of absolute-path syntax a string uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathStyle {
    /// POSIX / Unix absolute path: `/home/me/proj`.
    Posix,
    /// Windows drive-letter path: `C:\work\proj`, `c:/work/proj`, or `c:`.
    WindowsDrive,
    /// Windows UNC path: `\\server\share\...`.
    WindowsUnc,
}

/// Detect the style from the first few characters of an already-scheme-
/// stripped path. Returns None when it isn't recognizably absolute.
fn detect_style(path: &str) -> Option<PathStyle> {
    let bytes = path.as_bytes();
    if path.starts_with("\\\\") || path.starts_with("//") {
        return Some(PathStyle::WindowsUnc);
    }
    // `c:` / `c:\` / `c:/...` — drive letter then colon.
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Some(PathStyle::WindowsDrive);
    }
    if path.starts_with('/') {
        return Some(PathStyle::Posix);
    }
    None
}

// ============================================================================
// Normalization
// ============================================================================

/// Normalize an absolute filesystem path or `file://` URI into the canonical
/// form used for binding comparisons.
///
/// Platform-agnostic — the output only depends on the input's syntax, not
/// on the host OS. Same input always yields the same output.
///
/// Rules:
///   * Strip `file://` / `file:///` scheme (tolerating an optional host).
///   * URL-decode percent escapes.
///   * On Windows-style paths:
///       - Lowercase the drive letter (`D:` → `d:`).
///       - Use `\` as the separator throughout (`d:/foo` → `d:\foo`).
///       - Strip trailing separators but keep `c:\` as the root form.
///   * On POSIX paths: strip trailing `/` but keep `/` alone.
///   * On empty input: return empty string (callers filter).
pub fn normalize_workspace_root(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }

    let decoded = strip_scheme_and_decode(input);

    // `file:///D:/foo` → after scheme strip + decode we have `/D:/foo`. The
    // leading `/` is a URI artifact, not part of the path — drop it so the
    // drive-letter detector can fire on the following byte.
    let cleaned = strip_leading_slash_before_drive(&decoded);

    match detect_style(&cleaned) {
        Some(PathStyle::Posix) => normalize_posix(&cleaned),
        Some(PathStyle::WindowsDrive) => normalize_windows_drive(&cleaned),
        Some(PathStyle::WindowsUnc) => normalize_windows_unc(&cleaned),
        None => {
            // Unrecognized / relative — return as-is (trimmed). Callers
            // that require an absolute path should use
            // [`validate_workspace_root`] instead of trusting normalization.
            cleaned.trim().to_string()
        }
    }
}

fn strip_scheme_and_decode(input: &str) -> String {
    let without_scheme = if let Some(rest) = input.strip_prefix("file://") {
        // Triple-slash form `file:///abs` → `rest` = `/abs`. Host form
        // `file://localhost/abs` → drop up to the first `/`.
        match rest.find('/') {
            Some(0) => rest.to_string(),
            Some(n) => rest[n..].to_string(),
            None => rest.to_string(),
        }
    } else {
        input.to_string()
    };

    urlencoding::decode(&without_scheme)
        .map(|s| s.into_owned())
        .unwrap_or(without_scheme)
}

fn strip_leading_slash_before_drive(path: &str) -> String {
    let rest = match path.strip_prefix('/') {
        Some(r) => r,
        None => return path.to_string(),
    };
    let bytes = rest.as_bytes();
    let looks_like_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if looks_like_drive {
        rest.to_string()
    } else {
        path.to_string()
    }
}

fn normalize_posix(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_windows_drive(path: &str) -> String {
    // Lowercase the drive letter.
    let mut chars: Vec<char> = path.chars().collect();
    if !chars.is_empty() && chars[0].is_ascii_alphabetic() {
        chars[0] = chars[0].to_ascii_lowercase();
    }
    let mut s: String = chars.into_iter().collect();

    // Convert every `/` to `\` for canonical Windows form.
    s = s.replace('/', "\\");

    // Trim trailing `\`, but keep `c:\` as a root form.
    let trimmed = s.trim_end_matches('\\');
    if trimmed.len() < 2 {
        return s;
    }
    // After trim, `c:` needs its trailing `\` back to remain absolute.
    if trimmed.ends_with(':') {
        format!("{trimmed}\\")
    } else {
        trimmed.to_string()
    }
}

fn normalize_windows_unc(path: &str) -> String {
    // `\\server\share\path` — normalize separators to `\` and strip trailing `\`.
    let s = path.replace('/', "\\");
    let trimmed = s.trim_end_matches('\\');
    // Preserve the leading `\\` prefix.
    if trimmed.len() < 2 {
        "\\\\".to_string()
    } else {
        trimmed.to_string()
    }
}

// ============================================================================
// Validation (for manual user input)
// ============================================================================

/// Validation outcome for a prospective workspace root, returned by
/// [`validate_workspace_root`]. The UI renders normalized in the success
/// case and `reason` in the failure case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRootValidation {
    /// Empty input — UI shouldn't show an error while the field is empty.
    Empty,
    /// Accepted; `normalized` is what the caller should persist/submit.
    Ok { normalized: String },
    /// Rejected; show `reason` to the user.
    Invalid { reason: String },
}

/// Validate a user-entered workspace root.
///
/// Applied on manual add/edit ONLY — roots reported by connected MCP
/// clients are trusted (they come from a live `roots/list` response via
/// `SessionRootsRegistry` and are normalized on insert).
///
/// Rules enforced, independent of the host OS:
///   * Non-empty after trim.
///   * Normalization must classify the input as a real absolute path
///     (POSIX, Windows drive, or Windows UNC).
///   * Not the filesystem root alone (`/`, `c:\`, `\\`) — binding that
///     captures every session defeats the purpose.
///   * Windows-style paths may not contain `<>:"|?*` or stray `:` outside
///     the drive-letter position — the OS forbids those in filenames so a
///     path that contains them can't correspond to a real folder.
pub fn validate_workspace_root(input: &str) -> WorkspaceRootValidation {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return WorkspaceRootValidation::Empty;
    }

    let normalized = normalize_workspace_root(trimmed);
    if normalized.is_empty() {
        return WorkspaceRootValidation::Invalid {
            reason: "Path is empty after normalization.".into(),
        };
    }

    let style = match detect_style(&normalized) {
        Some(s) => s,
        None => {
            return WorkspaceRootValidation::Invalid {
                reason: "Path must be absolute (e.g. /home/me/proj or D:\\work\\proj). \
                     Relative paths can't route."
                    .into(),
            };
        }
    };

    if is_filesystem_root(&normalized, style) {
        return WorkspaceRootValidation::Invalid {
            reason:
                "Can't bind the filesystem root — every session would match. Pick a project folder."
                    .into(),
        };
    }

    if matches!(style, PathStyle::WindowsDrive | PathStyle::WindowsUnc) {
        if let Err(reason) = check_windows_reserved_chars(&normalized) {
            return WorkspaceRootValidation::Invalid { reason };
        }
    }

    WorkspaceRootValidation::Ok { normalized }
}

fn is_filesystem_root(normalized: &str, style: PathStyle) -> bool {
    match style {
        PathStyle::Posix => normalized == "/",
        PathStyle::WindowsDrive => {
            // `c:\` — 3 chars, drive letter + colon + backslash.
            normalized.len() == 3 && normalized.ends_with(":\\")
        }
        PathStyle::WindowsUnc => normalized == "\\\\",
    }
}

fn check_windows_reserved_chars(path: &str) -> Result<(), String> {
    const RESERVED: &[char] = &['<', '>', '"', '|', '?', '*'];
    // Byte index 1 is the drive-letter colon (`c:`); that's the only place
    // `:` is legal. Everywhere else it's a reserved character.
    for (i, ch) in path.char_indices() {
        if i == 1 && ch == ':' {
            continue;
        }
        if ch == ':' {
            return Err(format!("Illegal character ':' in path at position {i}."));
        }
        if RESERVED.contains(&ch) {
            return Err(format!(
                "Illegal character '{ch}' — Windows forbids {} in filenames.",
                RESERVED
                    .iter()
                    .map(|c| format!("'{c}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    Ok(())
}

// ============================================================================
// Longest-prefix match (separator-agnostic)
// ============================================================================

/// Returns the `workspace_root` in `candidates` whose path is the longest
/// prefix of `query`, respecting path-component boundaries.
///
/// Both `query` and every candidate MUST be already normalized via
/// [`normalize_workspace_root`]. The boundary check accepts either `/` or
/// `\` regardless of host OS so a binding written on Windows matches a
/// Linux reader (and vice versa).
pub fn longest_prefix_match<'a, I>(query: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut best: Option<&'a str> = None;
    for candidate in candidates {
        let matches = query == candidate
            || (query.starts_with(candidate)
                && query
                    .as_bytes()
                    .get(candidate.len())
                    .is_some_and(|b| *b == b'/' || *b == b'\\'));
        if matches && best.map(|b| candidate.len() > b.len()).unwrap_or(true) {
            best = Some(candidate);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- normalize -------------------------------------------------------

    #[test]
    fn normalize_posix_plain() {
        assert_eq!(
            normalize_workspace_root("/home/user/proj"),
            "/home/user/proj"
        );
    }

    #[test]
    fn normalize_posix_trailing_slash() {
        assert_eq!(
            normalize_workspace_root("/home/user/proj/"),
            "/home/user/proj"
        );
    }

    #[test]
    fn normalize_posix_file_uri() {
        assert_eq!(
            normalize_workspace_root("file:///home/user/proj"),
            "/home/user/proj"
        );
    }

    #[test]
    fn normalize_windows_plain_on_any_host() {
        // Normalization runs the same everywhere — cfg(windows) isn't involved.
        assert_eq!(
            normalize_workspace_root("D:\\Projects\\Foo"),
            "d:\\Projects\\Foo"
        );
        assert_eq!(normalize_workspace_root("C:/work/proj"), "c:\\work\\proj");
    }

    #[test]
    fn normalize_windows_file_uri_on_any_host() {
        assert_eq!(
            normalize_workspace_root("file:///D:/Projects/Foo"),
            "d:\\Projects\\Foo"
        );
    }

    #[test]
    fn normalize_windows_drive_letter_case_insensitive() {
        assert_eq!(
            normalize_workspace_root("D:\\Projects\\Foo"),
            normalize_workspace_root("d:\\Projects\\Foo")
        );
    }

    #[test]
    fn normalize_windows_trailing_sep() {
        assert_eq!(normalize_workspace_root("D:\\work\\"), "d:\\work");
        assert_eq!(normalize_workspace_root("D:\\"), "d:\\");
        assert_eq!(normalize_workspace_root("D:"), "d:\\");
    }

    #[test]
    fn normalize_unc_basic() {
        assert_eq!(
            normalize_workspace_root("\\\\server\\share\\folder"),
            "\\\\server\\share\\folder"
        );
        assert_eq!(
            normalize_workspace_root("\\\\server\\share\\folder\\"),
            "\\\\server\\share\\folder"
        );
    }

    #[test]
    fn normalize_percent_decoded() {
        let n = normalize_workspace_root("file:///home/user/my%20project");
        assert_eq!(n, "/home/user/my project");
    }

    // ---- validate --------------------------------------------------------

    #[test]
    fn validate_empty_is_not_an_error() {
        assert_eq!(validate_workspace_root(""), WorkspaceRootValidation::Empty);
        assert_eq!(
            validate_workspace_root("   "),
            WorkspaceRootValidation::Empty
        );
    }

    #[test]
    fn validate_accepts_posix() {
        assert_eq!(
            validate_workspace_root("/home/me/proj"),
            WorkspaceRootValidation::Ok {
                normalized: "/home/me/proj".into()
            }
        );
    }

    #[test]
    fn validate_accepts_windows_on_any_host() {
        assert_eq!(
            validate_workspace_root("D:\\proj"),
            WorkspaceRootValidation::Ok {
                normalized: "d:\\proj".into()
            }
        );
        assert_eq!(
            validate_workspace_root("c:/work/proj/"),
            WorkspaceRootValidation::Ok {
                normalized: "c:\\work\\proj".into()
            }
        );
    }

    #[test]
    fn validate_accepts_unc() {
        assert_eq!(
            validate_workspace_root("\\\\server\\share\\folder"),
            WorkspaceRootValidation::Ok {
                normalized: "\\\\server\\share\\folder".into()
            }
        );
    }

    #[test]
    fn validate_accepts_both_file_uris() {
        assert_eq!(
            validate_workspace_root("file:///home/me/proj"),
            WorkspaceRootValidation::Ok {
                normalized: "/home/me/proj".into()
            }
        );
        assert_eq!(
            validate_workspace_root("file:///D:/proj"),
            WorkspaceRootValidation::Ok {
                normalized: "d:\\proj".into()
            }
        );
    }

    #[test]
    fn validate_rejects_relative() {
        assert!(matches!(
            validate_workspace_root("my-project"),
            WorkspaceRootValidation::Invalid { .. }
        ));
        assert!(matches!(
            validate_workspace_root("./proj"),
            WorkspaceRootValidation::Invalid { .. }
        ));
        assert!(matches!(
            validate_workspace_root("~/proj"),
            WorkspaceRootValidation::Invalid { .. }
        ));
    }

    #[test]
    fn validate_rejects_filesystem_root() {
        for bad in &["/", "D:\\", "d:\\", "\\\\"] {
            match validate_workspace_root(bad) {
                WorkspaceRootValidation::Invalid { reason } => {
                    assert!(
                        reason.to_lowercase().contains("filesystem root"),
                        "got {reason}"
                    );
                }
                other => panic!("expected Invalid for {bad:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn validate_rejects_windows_reserved_chars() {
        match validate_workspace_root("D:\\bad|name") {
            WorkspaceRootValidation::Invalid { reason } => {
                assert!(reason.contains('|'), "got {reason}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
        match validate_workspace_root("D:\\has<bracket") {
            WorkspaceRootValidation::Invalid { reason } => {
                assert!(reason.contains('<'), "got {reason}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    // ---- longest_prefix_match — cross-platform ---------------------------

    #[test]
    fn longest_prefix_posix() {
        let bindings = ["/a", "/a/b", "/a/b/c"];
        assert_eq!(longest_prefix_match("/a/b/c", bindings), Some("/a/b/c"));
        assert_eq!(longest_prefix_match("/a/b/c/d", bindings), Some("/a/b/c"));
        assert_eq!(longest_prefix_match("/a/b", bindings), Some("/a/b"));
    }

    #[test]
    fn longest_prefix_windows_runs_on_any_host() {
        // No cfg(windows) gating — this test must pass on Linux CI too.
        let bindings = ["d:\\work", "d:\\work\\proj"];
        assert_eq!(
            longest_prefix_match("d:\\work\\proj\\src", bindings),
            Some("d:\\work\\proj")
        );
        assert_eq!(
            longest_prefix_match("d:\\work\\other", bindings),
            Some("d:\\work")
        );
    }

    #[test]
    fn longest_prefix_no_false_partial() {
        let bindings = ["/a/b"];
        assert_eq!(longest_prefix_match("/a/b-extra", bindings), None);
        let win = ["d:\\work"];
        assert_eq!(longest_prefix_match("d:\\workspace", win), None);
    }

    #[test]
    fn longest_prefix_empty_candidates() {
        let bindings: [&str; 0] = [];
        assert_eq!(longest_prefix_match("/a", bindings), None);
    }
}
