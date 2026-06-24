//! Shared guards and version comparison for package update policy (probe + resolution).

use mcpmux_core::UpdatePolicy;

/// Returns true for npm dist-tags that do not pin an exact semver.
pub fn is_floating_npm_tag(tag: &str) -> bool {
    matches!(
        tag.trim()
            .trim_start_matches('@')
            .to_ascii_lowercase()
            .as_str(),
        "latest" | "*" | "next" | "beta" | "canary" | "stable" | "release"
    )
}

/// Returns true when `version` matches strict semver (major.minor.patch).
pub fn is_valid_semver(version: &str) -> bool {
    let version = version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('=');
    if version.is_empty() {
        return false;
    }

    let core = version.split('+').next().unwrap_or(version);
    let (core, prerelease) = match core.split_once('-') {
        Some((core, pre)) if !pre.is_empty() => {
            if !is_valid_prerelease(pre) {
                return false;
            }
            (core, Some(pre))
        }
        Some((_, _)) => return false,
        None => (core, None),
    };
    let _ = prerelease;

    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return false;
    }

    parts.iter().all(|part| is_valid_numeric_ident(part))
}

/// Returns true when the server update policy locks to a pinned version.
pub fn is_pinned(policy: UpdatePolicy) -> bool {
    policy == UpdatePolicy::Pinned
}

/// Returns true when a background probe should report `update_available`.
pub fn probe_update_available(
    current: Option<&str>,
    latest: Option<&str>,
    npm_package_version: Option<&str>,
) -> bool {
    if npm_package_version.is_some_and(is_floating_npm_tag) {
        return false;
    }

    let Some(latest) = latest.filter(|value| !value.is_empty()) else {
        return false;
    };

    is_newer_than(latest, current)
}

/// Returns true when `latest` is strictly newer than `current`.
pub fn is_newer_than(latest: &str, current: Option<&str>) -> bool {
    let Some(current) = current.filter(|value| !value.is_empty()) else {
        return false;
    };

    let latest_parts = parse_version_parts(latest);
    let current_parts = parse_version_parts(current);
    latest_parts > current_parts || (latest_parts == current_parts && latest != current)
}

fn is_valid_numeric_ident(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if value == "0" {
        return true;
    }
    !value.starts_with('0') && value.chars().all(|ch| ch.is_ascii_digit())
}

fn is_valid_prerelease(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
}

/// Split a semver-ish string into numeric comparison parts.
fn parse_version_parts(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('=')
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_semver_accepts_release_and_prerelease() {
        assert!(is_valid_semver("1.2.3"));
        assert!(is_valid_semver("v1.2.3"));
        assert!(is_valid_semver("0.0.1"));
        assert!(is_valid_semver("1.2.3-beta.1"));
        assert!(is_valid_semver("1.2.3+build.1"));
    }

    #[test]
    fn is_valid_semver_rejects_loose_values() {
        assert!(!is_valid_semver("latest"));
        assert!(!is_valid_semver("1.2"));
        assert!(!is_valid_semver("01.2.3"));
        assert!(!is_valid_semver(""));
    }

    #[test]
    fn is_floating_npm_tag_recognizes_dist_tags() {
        assert!(is_floating_npm_tag("latest"));
        assert!(is_floating_npm_tag("@next"));
        assert!(!is_floating_npm_tag("1.2.3"));
    }

    #[test]
    fn probe_update_available_honors_floating_tag_and_unknown_current() {
        assert!(!probe_update_available(None, Some("2.0.0"), Some("latest"),));
        assert!(!probe_update_available(None, Some("2.0.0"), None));
        assert!(probe_update_available(Some("1.0.0"), Some("2.0.0"), None,));
    }
}
