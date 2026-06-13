//! WorkspaceBinding entity — maps a workspace root on disk to one or more
//! FeatureSets within a Space.
//!
//! Bindings are the only override surface for FS resolution:
//!
//!   workspace root matches a binding?  →  (binding.space_id, binding.feature_set_ids)
//!                                  else  →  deny (live session would hit
//!                                                 PendingRoots / WorkspaceNeedsBinding)
//!
//! A binding may resolve to multiple FeatureSets — the resolver hands them
//! all to `FeatureService::get_*_for_grants` which composes the union.
//! This is what lets one folder layer e.g. `Read Only` + `Project-specific
//! tools` without forcing the user to merge them into a single FS by hand.
//! Empty `feature_set_ids` is allowed — a "no Space tools" mapping: the
//! folder still routes to its Space (built-in servers apply per Space).
//!
//! Resolution is an EXACT match on the normalized root — there is no
//! ancestor/prefix inheritance (`d:\a\b` does not pick up a binding on
//! `d:\a`). Path handling is still **platform-agnostic**: a binding written
//! on Windows (`d:\work\proj`) must match on a Linux host reading the DB and
//! vice versa, so we normalize from the string's own style (drive-letter ⇒
//! Windows, leading `/` ⇒ POSIX) rather than from `cfg!(windows)`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A binding between a normalized workspace root and the FeatureSet(s) it
/// resolves to. `feature_set_ids` MAY be empty — an empty list is a valid
/// "no Space tools" mapping (the folder still routes to this Space; built-in
/// servers apply per Space).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceBinding {
    pub id: Uuid,
    pub workspace_root: String,
    pub space_id: Uuid,
    /// Order matters for UI rendering only — the resolver treats them as
    /// a set. Stored in the `workspace_binding_feature_sets` junction
    /// table (one row per FS, `sort_order` from this Vec's index).
    pub feature_set_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkspaceBinding {
    /// Convenience for the common single-FS case.
    pub fn new(
        workspace_root: impl Into<String>,
        space_id: Uuid,
        feature_set_id: impl Into<String>,
    ) -> Self {
        Self::new_multi(workspace_root, space_id, vec![feature_set_id.into()])
    }

    /// Construct a binding with zero or more FeatureSets. An empty list is
    /// allowed and persists as a "no Space tools" mapping.
    pub fn new_multi(
        workspace_root: impl Into<String>,
        space_id: Uuid,
        feature_set_ids: Vec<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            workspace_root: workspace_root.into(),
            space_id,
            feature_set_ids,
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
///   * Strip `file://` / `file:///` scheme (case-insensitive, tolerating an
///     optional host) and percent-decode — but ONLY when a scheme was
///     actually present, so the function is idempotent on plain paths that
///     contain a literal `%xx` (e.g. a folder named `proj%20demo`).
///   * On Windows-style paths:
///       - Case-fold the WHOLE path (Windows filesystems are
///         case-insensitive, so `D:\Foo` and `d:\foo` are the same folder).
///       - Use `\` as the separator throughout (`d:/foo` → `d:\foo`).
///       - Strip trailing separators but keep `c:\` as the root form.
///   * On POSIX paths: strip trailing `/` but keep `/` alone (case-sensitive).
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
    // Match `file://` case-insensitively (RFC 3986 schemes are
    // case-insensitive) without allocating unless it actually matches.
    let scheme_len = input
        .get(..7)
        .filter(|p| p.eq_ignore_ascii_case("file://"))
        .map(|_| 7);

    let Some(scheme_len) = scheme_len else {
        // No scheme: NOT a URI — return verbatim. Crucially we do NOT
        // percent-decode here, so re-normalizing an already-normalized plain
        // path (or one whose folder name legitimately contains `%xx`) is a
        // no-op. (Idempotency: normalize(normalize(x)) == normalize(x).)
        return input.to_string();
    };

    let rest = &input[scheme_len..];
    let without_scheme = reconstruct_uri_path(rest);

    // Scheme WAS present, so percent escapes are URI encoding — decode them.
    urlencoding::decode(&without_scheme)
        .map(|s| s.into_owned())
        .unwrap_or(without_scheme)
}

/// Turn the part of a `file://` URI after the scheme into a filesystem path,
/// preserving drive letters and UNC hosts that the naive "drop everything
/// before the first slash" approach used to discard.
fn reconstruct_uri_path(rest: &str) -> String {
    // Triple-slash form `file:///abs` → rest = `/abs`: no host component.
    if rest.starts_with('/') {
        return rest.to_string();
    }

    // Authority form `file://<host>[/<path>]`. Split off the host.
    let (host, path) = match rest.find('/') {
        Some(n) => (&rest[..n], &rest[n..]), // path keeps its leading '/'
        None => (rest, ""),
    };

    // `file://C:/Users/x` — the "host" is really a drive letter. Keep it.
    let host_bytes = host.as_bytes();
    let host_is_drive =
        host_bytes.len() == 2 && host_bytes[0].is_ascii_alphabetic() && host_bytes[1] == b':';
    if host_is_drive {
        return format!("{host}{path}");
    }

    // Empty or local host → ordinary local path (`file:///abs` equivalent).
    if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
        return if path.is_empty() {
            "/".to_string()
        } else {
            path.to_string()
        };
    }

    // A real remote host → UNC path `\\host\share\...`. Emit the `\\host`
    // prefix; normalize_windows_unc converts the remaining separators.
    format!("\\\\{host}{path}")
}

fn strip_leading_slash_before_drive(path: &str) -> String {
    // Strip ALL leading separators before a drive letter, not just one: a
    // `file://` URI for a Windows path can arrive with a doubled slash
    // (`file:////D:/x` → `//D:/x`), which `detect_style` would otherwise read
    // as a UNC path and mangle to `\\d:\x`. A genuine UNC path
    // (`//server/share`) has a non-drive first component, so trimming then
    // checking for `X:` leaves it untouched.
    let trimmed = path.trim_start_matches(['/', '\\']);
    let bytes = trimmed.as_bytes();
    let looks_like_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if looks_like_drive {
        trimmed.to_string()
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
    // Case-fold the WHOLE path, not just the drive letter: Windows
    // filesystems are case-insensitive, and binding lookup is exact string
    // equality, so `D:\Projects\Foo` and `d:\projects\foo` must collapse to
    // one key or the binding silently never matches the session root.
    let mut s = path.to_lowercase();

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
    // `\\server\share\path` — case-fold (UNC server/share names are
    // case-insensitive, same rationale as drive paths), normalize separators
    // to `\`, and strip the trailing `\`.
    let s = path.to_lowercase().replace('/', "\\");
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
        // The WHOLE path is case-folded (Windows is case-insensitive), not
        // just the drive letter, so bindings match regardless of casing.
        assert_eq!(
            normalize_workspace_root("D:\\Projects\\Foo"),
            "d:\\projects\\foo"
        );
        assert_eq!(normalize_workspace_root("C:/Work/Proj"), "c:\\work\\proj");
    }

    #[test]
    fn normalize_windows_file_uri_on_any_host() {
        assert_eq!(
            normalize_workspace_root("file:///D:/Projects/Foo"),
            "d:\\projects\\foo"
        );
    }

    #[test]
    fn normalize_windows_case_insensitive_full_path() {
        // Same folder, different casing anywhere in the path → one key.
        assert_eq!(
            normalize_workspace_root("D:\\Projects\\Foo"),
            normalize_workspace_root("d:\\PROJECTS\\foo")
        );
        // UNC server/share names are case-insensitive too.
        assert_eq!(
            normalize_workspace_root("\\\\SERVER\\Share\\Dir"),
            normalize_workspace_root("\\\\server\\share\\dir")
        );
    }

    #[test]
    fn normalize_is_idempotent() {
        // normalize(normalize(x)) == normalize(x) for every shape, including
        // plain paths whose folder name legitimately contains a `%xx` (must
        // NOT be percent-decoded when there was no file:// scheme).
        for input in [
            "/home/user/proj",
            "/home/user/my%20proj",
            "D:\\Projects\\Foo",
            "C:/work/proj/",
            "\\\\server\\share\\dir",
            "file:///D:/Projects/My%20App",
            "file:///home/user/my%20project",
        ] {
            let once = normalize_workspace_root(input);
            let twice = normalize_workspace_root(&once);
            assert_eq!(once, twice, "not idempotent for {input:?}");
        }
    }

    #[test]
    fn normalize_plain_percent_is_not_decoded() {
        // A real folder named `proj%20demo` (no scheme) keeps its literal %.
        assert_eq!(
            normalize_workspace_root("d:\\proj%20demo"),
            "d:\\proj%20demo"
        );
        assert_eq!(
            normalize_workspace_root("/home/user/proj%20demo"),
            "/home/user/proj%20demo"
        );
    }

    #[test]
    fn normalize_file_uri_scheme_case_insensitive() {
        assert_eq!(
            normalize_workspace_root("FILE:///home/user/proj"),
            "/home/user/proj"
        );
    }

    #[test]
    fn normalize_file_uri_drive_letter_host() {
        // Nonstandard `file://C:/...` — the "host" is really a drive letter.
        assert_eq!(
            normalize_workspace_root("file://C:/Users/x"),
            "c:\\users\\x"
        );
    }

    #[test]
    fn normalize_file_uri_localhost_host() {
        assert_eq!(
            normalize_workspace_root("file://localhost/home/user/proj"),
            "/home/user/proj"
        );
        assert_eq!(
            normalize_workspace_root("file://localhost/D:/work"),
            "d:\\work"
        );
    }

    #[test]
    fn normalize_file_uri_unc_host_reconstructed() {
        // Standard UNC file URI keeps the host as the UNC server.
        assert_eq!(
            normalize_workspace_root("file://server/share/dir"),
            "\\\\server\\share\\dir"
        );
    }

    #[test]
    fn normalize_doubled_slash_before_drive_is_not_unc() {
        // Regression (live manual test): a doubled leading slash before a
        // drive letter — from a `file:////D:/x` URI or a `//D:/x` path — must
        // collapse to a drive path, NOT be misread as a UNC path `\\d:\x`
        // (which then never matches the `d:\x` other clients report).
        let expected = "d:\\mcpmux\\mcp-mux";
        assert_eq!(normalize_workspace_root("//d:/mcpmux/mcp-mux"), expected);
        assert_eq!(
            normalize_workspace_root("\\\\d:\\mcpmux\\mcp-mux"),
            expected
        );
        assert_eq!(
            normalize_workspace_root("file:////D:/mcpmux/mcp-mux"),
            expected
        );
        // All collapse to the SAME key as the canonical drive forms.
        assert_eq!(normalize_workspace_root("D:\\mcpmux\\mcp-mux"), expected);
        assert_eq!(
            normalize_workspace_root("file:///D:/mcpmux/mcp-mux"),
            expected
        );
        // A genuine UNC path (non-drive first component) is left intact.
        assert_eq!(
            normalize_workspace_root("//server/share"),
            "\\\\server\\share"
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
}
