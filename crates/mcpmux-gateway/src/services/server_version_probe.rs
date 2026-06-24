//! Background version probe for notify/auto server update policies.
//!
//! Shells out to `npm view`, `uv tool list`, and `uv tool list --outdated`;
//! queries the PyPI JSON API for uvx latest versions; caches results on
//! `installed_servers`, and emits `ServerUpdateAvailable` domain events.

use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::pool::transport::resolution::npx_cache_resolved_version;
use crate::services::package_version::{
    is_floating_npm_tag, is_valid_semver, probe_update_available,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use mcpmux_core::{
    AppSettingsRepository, DomainEvent, EventBus, InstalledServer, InstalledServerRepository,
    TransportConfig, UpdatePolicy,
};
use tokio::time;
use tracing::{debug, info, warn};
use uuid::Uuid;

const DEFAULT_PROBE_INTERVAL_HOURS: u64 = 6;
const PROBE_INTERVAL_HOURS_KEY: &str = "servers.version_probe_interval_hours";
const LAST_VERSION_PROBE_AT_KEY: &str = "servers.last_version_probe_at";
const PROBE_CONCURRENCY: usize = 4;

/// Result of probing one installed server.
#[derive(Debug, Clone)]
pub struct ServerVersionProbeResult {
    pub space_id: String,
    pub server_id: String,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub checked_at: DateTime<Utc>,
}

/// Summary returned by bulk probe operations.
#[derive(Debug, Clone, Default)]
pub struct ServerVersionProbeSummary {
    pub checked: usize,
    pub updates_available: usize,
    pub checked_at: DateTime<Utc>,
}

/// Probes npm/PyPI for package updates and persists notify-mode cache columns.
#[derive(Clone)]
pub struct ServerVersionProbeService {
    installed_server_repo: Arc<dyn InstalledServerRepository>,
    settings_repo: Arc<dyn AppSettingsRepository>,
    event_bus: Arc<EventBus>,
    scheduler_started: Arc<AtomicBool>,
}

impl ServerVersionProbeService {
    /// Build a probe service wired to storage and the application event bus.
    pub fn new(
        installed_server_repo: Arc<dyn InstalledServerRepository>,
        settings_repo: Arc<dyn AppSettingsRepository>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            installed_server_repo,
            settings_repo,
            event_bus,
            scheduler_started: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the startup + interval background scheduler (idempotent).
    pub fn start_scheduler(self: Arc<Self>) {
        if self
            .scheduler_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        tokio::spawn(async move {
            info!("[VersionProbe] Running startup version probe");
            if let Err(error) = self.probe_all().await {
                warn!("[VersionProbe] Startup probe failed: {error}");
            }

            loop {
                let interval = self.probe_interval().await;
                time::sleep(interval).await;
                debug!("[VersionProbe] Running scheduled version probe");
                if let Err(error) = self.probe_all().await {
                    warn!("[VersionProbe] Scheduled probe failed: {error}");
                }
            }
        });
    }

    /// Probe every notify/auto package-managed server.
    pub async fn probe_all(&self) -> Result<ServerVersionProbeSummary> {
        let servers: Vec<InstalledServer> = self
            .installed_server_repo
            .list()
            .await?
            .into_iter()
            .filter(Self::is_probe_eligible)
            .collect();

        let uv_outdated = tokio::task::spawn_blocking(fetch_uv_outdated_map)
            .await
            .ok()
            .flatten();
        let uv_tool_list = tokio::task::spawn_blocking(fetch_uv_tool_list_map)
            .await
            .ok()
            .flatten();
        let checked_at = Utc::now();

        let results = stream::iter(servers)
            .map(|server| {
                let service = self.clone();
                let uv_outdated = uv_outdated.clone();
                let uv_tool_list = uv_tool_list.clone();
                async move {
                    service
                        .probe_installed_server(
                            &server,
                            uv_outdated.as_ref(),
                            uv_tool_list.as_ref(),
                            checked_at,
                        )
                        .await
                }
            })
            .buffer_unordered(PROBE_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

        let mut summary = ServerVersionProbeSummary {
            checked_at,
            ..Default::default()
        };

        for result in results {
            match result {
                Ok(probe_result) => {
                    summary.checked += 1;
                    if probe_result.update_available {
                        summary.updates_available += 1;
                    }
                }
                Err(error) => {
                    warn!("[VersionProbe] Failed probing server: {error}");
                }
            }
        }

        self.settings_repo
            .set(LAST_VERSION_PROBE_AT_KEY, &checked_at.to_rfc3339())
            .await
            .context("failed to persist last version probe timestamp")?;

        Ok(summary)
    }

    /// Probe a single installed server by registry id within a space.
    pub async fn probe_server(
        &self,
        space_id: &str,
        server_id: &str,
    ) -> Result<ServerVersionProbeResult> {
        let server = self
            .installed_server_repo
            .get_by_server_id(space_id, server_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Server not found: {space_id}/{server_id}"))?;

        if !Self::is_probe_eligible(&server) {
            anyhow::bail!("Server transport is not package-managed (npx/uvx only)");
        }

        let uv_outdated = tokio::task::spawn_blocking(fetch_uv_outdated_map)
            .await
            .ok()
            .flatten();
        let uv_tool_list = tokio::task::spawn_blocking(fetch_uv_tool_list_map)
            .await
            .ok()
            .flatten();
        let checked_at = Utc::now();
        self.probe_installed_server(
            &server,
            uv_outdated.as_ref(),
            uv_tool_list.as_ref(),
            checked_at,
        )
        .await
    }

    async fn probe_interval(&self) -> Duration {
        let hours = match self.settings_repo.get(PROBE_INTERVAL_HOURS_KEY).await {
            Ok(Some(value)) => value.parse::<u64>().unwrap_or(DEFAULT_PROBE_INTERVAL_HOURS),
            _ => DEFAULT_PROBE_INTERVAL_HOURS,
        };
        Duration::from_secs(hours.max(1) * 3600)
    }

    fn is_probe_eligible(server: &InstalledServer) -> bool {
        matches!(
            server.update_policy,
            UpdatePolicy::Notify | UpdatePolicy::Auto
        ) && package_spec(server).is_some()
    }

    async fn probe_installed_server(
        &self,
        server: &InstalledServer,
        uv_outdated: Option<&HashMap<String, UvOutdatedEntry>>,
        uv_tool_list: Option<&HashMap<String, String>>,
        checked_at: DateTime<Utc>,
    ) -> Result<ServerVersionProbeResult> {
        let Some(spec) = package_spec(server) else {
            anyhow::bail!("No resolvable package for {}", server.server_id);
        };

        let current_version = current_version(server, &spec, uv_tool_list).await;
        let latest_version = match spec.transport_kind {
            PackageTransportKind::Npx => {
                let package_name = spec.package_name.clone();
                tokio::task::spawn_blocking(move || fetch_npm_latest_version(&package_name))
                    .await
                    .ok()
                    .flatten()
            }
            PackageTransportKind::Uvx => {
                let outdated_latest = uv_outdated
                    .and_then(|map| map.get(&spec.package_name))
                    .map(|entry| entry.latest.clone());
                if outdated_latest.is_some() {
                    outdated_latest
                } else {
                    fetch_pypi_latest_version(&spec.package_name).await
                }
            }
        };

        self.installed_server_repo
            .update_version_cache(
                &server.id,
                latest_version.clone(),
                current_version.clone(),
                checked_at,
            )
            .await?;

        let npm_package_version = npm_package_version_suffix(server, &spec);
        let update_available = probe_update_available(
            current_version.as_deref(),
            latest_version.as_deref(),
            npm_package_version.as_deref(),
        );

        if update_available {
            let space_uuid = Uuid::parse_str(&server.space_id)
                .with_context(|| format!("Invalid space_id: {}", server.space_id))?;
            self.event_bus
                .sender()
                .emit(DomainEvent::ServerUpdateAvailable {
                    space_id: space_uuid,
                    server_id: server.server_id.clone(),
                    current_version: current_version.clone(),
                    latest_version: latest_version.clone(),
                });
        }

        Ok(ServerVersionProbeResult {
            space_id: server.space_id.clone(),
            server_id: server.server_id.clone(),
            current_version,
            latest_version,
            update_available,
            checked_at,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageTransportKind {
    Npx,
    Uvx,
}

#[derive(Debug, Clone)]
struct PackageSpec {
    transport_kind: PackageTransportKind,
    package_name: String,
}

#[derive(Debug, Clone)]
struct UvOutdatedEntry {
    latest: String,
}

/// Resolve the npm/PyPI package name for an installed stdio server.
fn package_spec(server: &InstalledServer) -> Option<PackageSpec> {
    let definition = server.get_definition()?;
    let TransportConfig::Stdio { command, args, .. } = definition.transport else {
        return None;
    };

    match command.as_str() {
        "npx" => find_npx_package_arg(&args).map(|package| PackageSpec {
            transport_kind: PackageTransportKind::Npx,
            package_name: strip_package_version(&package),
        }),
        "uvx" | "uv" => extract_uv_package_name(&command, &args).map(|package| PackageSpec {
            transport_kind: PackageTransportKind::Uvx,
            package_name: package,
        }),
        _ => None,
    }
}

/// Version suffix from the npx package argument, when present.
fn npm_package_version_suffix(server: &InstalledServer, spec: &PackageSpec) -> Option<String> {
    if spec.transport_kind != PackageTransportKind::Npx {
        return None;
    }

    let definition = server.get_definition()?;
    let TransportConfig::Stdio { args, .. } = definition.transport else {
        return None;
    };

    find_npx_package_arg(&args).and_then(|package| split_npm_package_arg(&package).1)
}

/// Best-effort current version: pin, package suffix, uv tool list, or uv arg pin.
async fn current_version(
    server: &InstalledServer,
    spec: &PackageSpec,
    uv_tool_list: Option<&HashMap<String, String>>,
) -> Option<String> {
    if let Some(pinned) = server.pinned_version.as_deref().filter(|v| !v.is_empty()) {
        return Some(pinned.to_string());
    }

    let definition = server.get_definition()?;
    let TransportConfig::Stdio { command, args, .. } = definition.transport else {
        return None;
    };

    match spec.transport_kind {
        PackageTransportKind::Npx => {
            let package_arg = find_npx_package_arg(&args)?;
            let bare_name = split_npm_package_arg(&package_arg).0;

            // After an explicit update, npm caches the new version under
            // `@pkg@{resolved_version}` (e.g. `@playwright/mcp@0.0.76`).
            // Build a versioned lookup using latest_available_version so the probe
            // finds the freshly cached entry even though the stored args still carry
            // the pre-update semver.
            let latest_versioned = server
                .latest_available_version
                .as_ref()
                .filter(|v| is_valid_semver(v))
                .map(|v| format!("{}@{}", bare_name, v));

            // Try original arg first (normal/cold-cache case), then the
            // latest-versioned spec (post-update case where old entry was evicted).
            let cache_version = tokio::task::spawn_blocking({
                let package_arg = package_arg.clone();
                move || {
                    npx_cache_resolved_version(&package_arg).or_else(|| {
                        latest_versioned
                            .as_deref()
                            .and_then(npx_cache_resolved_version)
                    })
                }
            })
            .await
            .ok()
            .flatten();

            if let Some(version) = cache_version {
                return Some(version);
            }

            // No cache hit: preserve the DB-stored current_version (set during an
            // explicit update) rather than clobbering it with the stale args semver.
            // Only fall back to args semver when the DB has nothing (cold-cache,
            // first-run before any update has ever run).
            server.current_version.clone().or_else(|| {
                split_npm_package_arg(&package_arg)
                    .1
                    .filter(|version| !is_floating_npm_tag(version))
                    .filter(|version| is_valid_semver(version))
                    .map(|version| version.to_string())
            })
        }
        PackageTransportKind::Uvx => uv_tool_list
            .and_then(|map| map.get(&spec.package_name))
            .cloned()
            .or_else(|| {
                find_uv_package_arg(&command, &args).and_then(|package| {
                    split_uv_version(&package)
                        .1
                        .filter(|version| is_valid_semver(version))
                        .map(|version| version.to_string())
                })
            }),
    }
}

/// Parse the latest published version from a PyPI JSON API body.
fn parse_pypi_json_version(body: &serde_json::Value) -> Option<String> {
    body.get("info")?
        .get("version")?
        .as_str()
        .filter(|version| !version.is_empty())
        .map(|version| version.to_string())
}

/// Fetch the latest published PyPI version via the JSON API.
async fn fetch_pypi_latest_version(package: &str) -> Option<String> {
    let url = format!(
        "https://pypi.org/pypi/{}/json",
        urlencoding::encode(package)
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;
    let response = client.get(&url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: serde_json::Value = response.json().await.ok()?;
    parse_pypi_json_version(&body)
}

/// Fetch the latest published version via `npm view <pkg> version`.
fn fetch_npm_latest_version(package: &str) -> Option<String> {
    let output = Command::new("npm")
        .args(["view", package, "version"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

/// Parse `uv tool list` into a package-name → installed-version map.
fn fetch_uv_tool_list_map() -> Option<HashMap<String, String>> {
    let output = Command::new("uv").args(["tool", "list"]).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let mut map = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((name, version)) = parse_uv_tool_list_line(line) {
            map.insert(name, version);
        }
    }
    Some(map)
}

/// Parse one `uv tool list` row (`<name> v<version>`).
fn parse_uv_tool_list_line(line: &str) -> Option<(String, String)> {
    let mut parts = line.split_whitespace();
    let name = parts.next()?.to_string();
    let version = parts.next()?.trim_start_matches('v').to_string();
    if version.is_empty() {
        return None;
    }
    Some((name, version))
}

/// Parse `uv tool list --outdated` into a package-name map.
fn fetch_uv_outdated_map() -> Option<HashMap<String, UvOutdatedEntry>> {
    let output = Command::new("uv")
        .args(["tool", "list", "--outdated"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let mut map = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(entry) = parse_uv_outdated_line(line) {
            map.insert(entry.0, entry.1);
        }
    }
    Some(map)
}

/// Parse one `uv tool list --outdated` row.
fn parse_uv_outdated_line(line: &str) -> Option<(String, UvOutdatedEntry)> {
    let mut parts = line.split_whitespace();
    let name = parts.next()?.to_string();
    let remainder = parts.collect::<Vec<_>>().join(" ");
    if remainder.contains("->") {
        let (_installed, latest) = remainder.split_once("->")?;
        return Some((
            name,
            UvOutdatedEntry {
                latest: latest.trim().trim_start_matches('v').to_string(),
            },
        ));
    }
    None
}

fn find_npx_package_arg(args: &[String]) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if matches!(arg, "-y" | "--yes") {
            let next = index + 1;
            if next < args.len() && !args[next].starts_with('-') {
                return Some(args[next].clone());
            }
        }
        index += 1;
    }

    args.iter()
        .find(|arg| !arg.starts_with('-') && arg.as_str() != "--")
        .cloned()
}

fn extract_uv_package_name(command: &str, args: &[String]) -> Option<String> {
    find_uv_package_arg(command, args).map(|package| split_uv_version(&package).0)
}

fn find_uv_package_arg(command: &str, args: &[String]) -> Option<String> {
    match command {
        "uvx" => args.iter().find(|arg| !arg.starts_with('-')).cloned(),
        "uv" if args.first().map(String::as_str) == Some("run") => {
            let mut index = 1;
            while index < args.len() {
                let arg = args[index].as_str();
                if arg.starts_with('-') {
                    if matches!(arg, "-m" | "--module") {
                        index += 2;
                        continue;
                    }
                    index += 1;
                    continue;
                }
                return Some(args[index].clone());
            }
            None
        }
        _ => None,
    }
}

fn strip_package_version(package: &str) -> String {
    split_npm_package_arg(package).0
}

fn split_npm_package_arg(package: &str) -> (String, Option<String>) {
    if let Some(scoped) = package.strip_prefix('@') {
        if let Some(at_idx) = scoped.find('@') {
            let split_at = 1 + at_idx;
            return (
                package[..split_at].to_string(),
                Some(package[split_at + 1..].to_string()),
            );
        }
        return (package.to_string(), None);
    }

    if let Some(at_idx) = package.rfind('@') {
        return (
            package[..at_idx].to_string(),
            Some(package[at_idx + 1..].to_string()),
        );
    }

    (package.to_string(), None)
}

fn split_uv_version(package: &str) -> (String, Option<String>) {
    if let Some((name, version)) = package.split_once("==") {
        return (name.to_string(), Some(version.to_string()));
    }
    (package.to_string(), None)
}

#[cfg(any(test, feature = "test-utils"))]
pub mod update_policy_parsing {
    /// Parse one `uv tool list` row (`<name> v<version>`).
    pub fn parse_uv_tool_list_line(line: &str) -> Option<(String, String)> {
        super::parse_uv_tool_list_line(line)
    }

    /// Parse one `uv tool list --outdated` row.
    pub fn parse_uv_outdated_line(line: &str) -> Option<(String, String)> {
        super::parse_uv_outdated_line(line).map(|(name, entry)| (name, entry.latest))
    }

    /// Parse the latest published version from a PyPI JSON API body.
    pub fn parse_pypi_json_version(body: &serde_json::Value) -> Option<String> {
        super::parse_pypi_json_version(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::package_version::is_newer_than;

    #[test]
    fn is_newer_than_compares_numeric_segments() {
        assert!(is_newer_than("1.2.0", Some("1.1.9")));
        assert!(!is_newer_than("1.2.0", Some("1.2.0")));
        assert!(!is_newer_than("2.0.0", None));
    }

    #[test]
    fn parse_uv_outdated_line_reads_arrow_format() {
        let (name, entry) =
            parse_uv_outdated_line("mcp-server v1.0.0 -> v1.2.0").expect("parse line");
        assert_eq!(name, "mcp-server");
        assert_eq!(entry.latest, "1.2.0");
    }

    #[test]
    fn parse_uv_tool_list_line_reads_name_and_version() {
        let (name, version) = parse_uv_tool_list_line("ruff v0.8.6").expect("parse line");
        assert_eq!(name, "ruff");
        assert_eq!(version, "0.8.6");
    }

    #[test]
    fn extract_uv_package_name_strips_pep508_version_pin() {
        let args = vec!["mcp-server-fetch==2025.1.17".to_string()];
        let name = extract_uv_package_name("uvx", &args).expect("extract name");
        assert_eq!(name, "mcp-server-fetch");

        let bare = vec!["mcp-server-fetch".to_string()];
        assert_eq!(
            extract_uv_package_name("uvx", &bare).as_deref(),
            Some("mcp-server-fetch")
        );
    }

    #[test]
    fn split_npm_package_arg_handles_scoped_packages() {
        let (name, version) = split_npm_package_arg("@scope/pkg@1.2.3");
        assert_eq!(name, "@scope/pkg");
        assert_eq!(version.as_deref(), Some("1.2.3"));
    }
}
