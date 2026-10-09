//! HTTP client for fetching server definitions from Registry API.
//!
//! This client uses the bundle-only strategy (see ADR-001).
//! All server discovery, filtering, and searching is done client-side
//! against the cached bundle data.
//!
//! Supports ETag-based conditional fetching to avoid re-downloading
//! unchanged bundles.
//!
//! Bundles from the official registry must be signed once
//! [`OFFICIAL_BUNDLE_KEYS`] lists a key: the response carries an Ed25519
//! signature over the exact bytes of its `data` member in the
//! [`BUNDLE_SIGNATURE_HEADER`] header (base64).

use anyhow::{Context as _, Result};
use base64::Engine as _;
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::domain::ServerDefinition;

/// The official registry.
pub const OFFICIAL_REGISTRY_URL: &str = "https://api.mcpmux.com";

/// Ed25519 public keys (base64 of the 32-byte key) the official registry
/// signs its bundle with. Empty until the registry publishes signed
/// bundles; once a key is listed, bundles from [`OFFICIAL_REGISTRY_URL`]
/// without a valid signature from one of them are rejected.
const OFFICIAL_BUNDLE_KEYS: &[&str] = &[];

/// Response header with the bundle signature: base64 of the Ed25519
/// signature over the exact bytes of the response's `data` member.
pub const BUNDLE_SIGNATURE_HEADER: &str = "x-mcpmux-bundle-signature";

// ============================================
// Bundle Types
// ============================================

/// Complete registry bundle from /v1/bundle
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(from = "RawRegistryBundle")]
pub struct RegistryBundle {
    pub version: String,
    pub updated_at: String,
    pub servers: Vec<ServerDefinition>,
    pub categories: Vec<Category>,
    pub ui: UiConfig,
    pub home: Option<HomeConfig>,
    /// Servers dropped while parsing because this client couldn't read them.
    /// Not serialized, so a disk-cached bundle reads back as 0.
    #[serde(skip)]
    pub skipped_servers: usize,
    /// The pinned key (base64) this client checked the bundle's signature
    /// against when fetching it, if any. Kept in the disk cache, so a cache
    /// saved before bundles had to be signed, or checked against a key that
    /// is no longer trusted, is not used once bundles must be signed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_by: Option<String>,
}

/// Wire form of [`RegistryBundle`], with each server left as raw JSON.
///
/// Servers are parsed one by one and any that fail are skipped. The registry
/// can publish schema values this client predates (a new auth type, badge,
/// transport, ...); one such server must not take down discovery for the
/// whole bundle.
#[derive(Deserialize)]
struct RawRegistryBundle {
    version: String,
    updated_at: String,
    servers: Vec<serde_json::Value>,
    categories: Vec<Category>,
    ui: UiConfig,
    home: Option<HomeConfig>,
    #[serde(default)]
    signed_by: Option<String>,
}

impl From<RawRegistryBundle> for RegistryBundle {
    fn from(raw: RawRegistryBundle) -> Self {
        let total = raw.servers.len();
        let servers: Vec<ServerDefinition> = raw
            .servers
            .into_iter()
            .filter_map(|value| {
                let id = value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<missing id>")
                    .to_string();
                match serde_json::from_value::<ServerDefinition>(value) {
                    Ok(server) => Some(server),
                    Err(e) => {
                        tracing::warn!("Skipping registry server '{}': {}", id, e);
                        None
                    }
                }
            })
            .collect();

        Self {
            version: raw.version,
            updated_at: raw.updated_at,
            skipped_servers: total - servers.len(),
            servers,
            categories: raw.categories,
            ui: raw.ui,
            home: raw.home,
            signed_by: raw.signed_by,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

// ============================================
// UI Configuration Types (API-driven)
// ============================================

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiConfig {
    pub filters: Vec<FilterDefinition>,
    pub sort_options: Vec<SortOption>,
    pub default_sort: String,
    pub items_per_page: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterDefinition {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub filter_type: String, // "single" or "multi"
    pub options: Vec<FilterOption>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterOption {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    #[serde(rename = "match")]
    pub match_rule: Option<FilterMatch>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterMatch {
    pub field: String,
    pub operator: String, // "eq", "in", "contains"
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SortOption {
    pub id: String,
    pub label: String,
    pub rules: Vec<SortRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SortRule {
    pub field: String,
    pub direction: String,     // "asc" or "desc"
    pub nulls: Option<String>, // "first" or "last"
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HomeConfig {
    pub featured_server_ids: Vec<String>,
}

// ============================================
// Fetch Result
// ============================================

/// Result of fetching a bundle with ETag support
#[derive(Debug)]
pub enum FetchBundleResult {
    /// New or updated bundle received
    Updated {
        bundle: Box<RegistryBundle>,
        etag: Option<String>,
    },
    /// Bundle unchanged (304 Not Modified)
    NotModified,
}

// ============================================
// Client Implementation
// ============================================

/// Client for fetching data from McpMux Registry API
///
/// This client only uses the bundle endpoint. Individual endpoints
/// (servers, categories) are not used per ADR-001.
pub struct RegistryApiClient {
    base_url: String,
    client: reqwest::Client,
    /// Ed25519 keys a bundle must be signed with; empty to accept unsigned
    /// bundles.
    bundle_keys: Vec<[u8; 32]>,
}

impl RegistryApiClient {
    /// Create a new Registry API client
    pub fn new(base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("McpMux/1.0")
            .build()
            .expect("Failed to build HTTP client");

        let bundle_keys = if is_official_registry(&base_url) {
            official_bundle_keys()
        } else {
            Vec::new()
        };
        Self {
            base_url,
            client,
            bundle_keys,
        }
    }

    /// Require bundles signed with one of `keys` (raw Ed25519 public keys).
    pub fn with_bundle_keys(mut self, keys: Vec<[u8; 32]>) -> Self {
        self.bundle_keys = keys;
        self
    }

    /// Whether fetched bundles must carry a valid signature.
    pub fn verifies_bundles(&self) -> bool {
        !self.bundle_keys.is_empty()
    }

    /// Whether `key` (base64, as in [`RegistryBundle::signed_by`]) is one of
    /// the keys bundles must be signed with.
    pub fn trusts_key(&self, key: &str) -> bool {
        self.bundle_keys.iter().any(|k| encode_key(k) == key)
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Fetch complete registry bundle from /v1/bundle
    ///
    /// If `current_etag` is provided, sends `If-None-Match` header.
    /// Returns `NotModified` if server responds with 304.
    ///
    /// This is the ONLY method used for fetching registry data.
    /// All filtering, searching, and sorting is done client-side.
    pub async fn fetch_bundle(&self, current_etag: Option<&str>) -> Result<FetchBundleResult> {
        let url = format!("{}/v1/bundle", self.base_url);

        tracing::info!("Fetching registry bundle from {}", url);

        let mut request = self.client.get(&url);

        // Add If-None-Match header if we have a cached ETag
        if let Some(etag) = current_etag {
            tracing::debug!("Sending If-None-Match: {}", etag);
            request = request.header("If-None-Match", etag);
        }

        let response = request
            .send()
            .await
            .context("Failed to send request to registry API")?;

        let status = response.status();

        // Handle 304 Not Modified
        if status == reqwest::StatusCode::NOT_MODIFIED {
            tracing::info!("Registry bundle not modified (304)");
            return Ok(FetchBundleResult::NotModified);
        }

        if !status.is_success() {
            anyhow::bail!("Registry API returned status: {}", status);
        }

        // Extract ETag from response headers
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let signature = response
            .headers()
            .get(BUNDLE_SIGNATURE_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);

        let body = read_capped(response, MAX_BUNDLE_BYTES)
            .await
            .context("Failed to read registry bundle response")?;
        let bundle = verify_and_parse(&body, signature.as_deref(), &self.bundle_keys)?;

        tracing::info!(
            "Fetched {} servers, {} filters, {} sort options (version: {}, updated: {}, etag: {:?})",
            bundle.servers.len(),
            bundle.ui.filters.len(),
            bundle.ui.sort_options.len(),
            bundle.version,
            bundle.updated_at,
            etag
        );

        Ok(FetchBundleResult::Updated {
            bundle: Box::new(bundle),
            etag,
        })
    }
}

/// Largest registry bundle accepted. The real bundle is far smaller; the
/// cap keeps a broken or hostile registry from exhausting memory.
const MAX_BUNDLE_BYTES: usize = 32 * 1024 * 1024;

/// Read a response body, failing once it exceeds `max` bytes.
async fn read_capped(mut response: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|len| len > max as u64)
    {
        anyhow::bail!("registry bundle is larger than {max} bytes");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > max {
            anyhow::bail!("registry bundle is larger than {max} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Whether `base_url` points at the official registry (by host, so case, a
/// port or the scheme don't change which keys apply).
fn is_official_registry(base_url: &str) -> bool {
    let official = reqwest::Url::parse(OFFICIAL_REGISTRY_URL)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string));
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .is_some_and(|host| Some(host) == official)
}

/// A public key as stored in [`RegistryBundle::signed_by`].
fn encode_key(key: &[u8; 32]) -> String {
    base64::engine::general_purpose::STANDARD.encode(key)
}

/// [`OFFICIAL_BUNDLE_KEYS`], decoded.
fn official_bundle_keys() -> Vec<[u8; 32]> {
    OFFICIAL_BUNDLE_KEYS
        .iter()
        .map(|key| {
            base64::engine::general_purpose::STANDARD
                .decode(key)
                .ok()
                .and_then(|key| <[u8; 32]>::try_from(key).ok())
                .expect("OFFICIAL_BUNDLE_KEYS holds base64 32-byte Ed25519 keys")
        })
        .collect()
}

/// Parse a `/v1/bundle` response body once, checking its signature (the
/// [`BUNDLE_SIGNATURE_HEADER`] value) over the exact bytes of its `data`
/// member against `keys` when there are any. The bundle records which key
/// matched; that is set here, never taken from the payload.
fn verify_and_parse(
    body: &[u8],
    signature: Option<&str>,
    keys: &[[u8; 32]],
) -> Result<RegistryBundle> {
    #[derive(Deserialize)]
    struct Envelope<'a> {
        #[serde(borrow)]
        data: &'a serde_json::value::RawValue,
    }

    let envelope: Envelope =
        serde_json::from_slice(body).context("Failed to parse registry bundle JSON")?;
    let signed_by = if keys.is_empty() {
        None
    } else {
        let signature = signature.context("registry bundle is not signed")?;
        let signature = base64::engine::general_purpose::STANDARD
            .decode(signature.trim())
            .context("registry bundle signature is not valid base64")?;
        let signed = envelope.data.get().as_bytes();
        let key = keys
            .iter()
            .find(|key| {
                UnparsedPublicKey::new(&ED25519, key)
                    .verify(signed, &signature)
                    .is_ok()
            })
            .context("registry bundle signature does not match a trusted key")?;
        Some(encode_key(key))
    };
    let mut bundle: RegistryBundle = serde_json::from_str(envelope.data.get())
        .context("Failed to parse registry bundle JSON")?;
    bundle.signed_by = signed_by;
    Ok(bundle)
}

/// Parse a `/v1/bundle` response body without checking a signature.
#[cfg(test)]
fn parse_bundle(body: &[u8]) -> Result<RegistryBundle> {
    verify_and_parse(body, None, &[])
}

/// A stand-in registry and signing helpers for tests.
#[cfg(test)]
pub(crate) mod test_registry {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair as _};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    /// A fresh Ed25519 key pair and its raw public key.
    pub(crate) fn key_pair() -> (Ed25519KeyPair, [u8; 32]) {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = <[u8; 32]>::try_from(pair.public_key().as_ref()).unwrap();
        (pair, public)
    }

    /// The `data` member of a bundle holding one server.
    pub(crate) fn bundle_data(server_id: &str) -> String {
        format!(
            r#"{{"version":"1.0.0","updated_at":"2026-10-01T00:00:00Z","servers":[{{"id":"{server_id}","name":"{server_id}","transport":{{"type":"stdio","command":"npx"}}}}],"categories":[],"ui":{{"filters":[],"sort_options":[],"default_sort":"name","items_per_page":20}},"home":null}}"#
        )
    }

    /// A `/v1/bundle` body around `data`, and `pair`'s signature of `data`.
    pub(crate) fn signed(pair: &Ed25519KeyPair, data: &str) -> (Vec<u8>, String) {
        let body = format!(r#"{{"data":{data},"meta":null}}"#).into_bytes();
        let signature =
            base64::engine::general_purpose::STANDARD.encode(pair.sign(data.as_bytes()));
        (body, signature)
    }

    /// What the stand-in registry answers, and what it was asked.
    #[derive(Default)]
    pub(crate) struct Served {
        pub body: Vec<u8>,
        pub signature: Option<String>,
        pub etag: Option<String>,
        /// Answer 503 instead.
        pub down: bool,
        /// Each request's If-None-Match value.
        pub if_none_match: Vec<Option<String>>,
    }

    /// A local HTTP server answering every request like `/v1/bundle`.
    pub(crate) struct MockRegistry {
        pub url: String,
        served: Arc<Mutex<Served>>,
    }

    impl MockRegistry {
        pub(crate) async fn start() -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let served = Arc::new(Mutex::new(Served::default()));
            let state = served.clone();
            tokio::spawn(async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let state = state.clone();
                    tokio::spawn(async move {
                        let mut request = Vec::new();
                        let mut buf = [0u8; 4096];
                        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                            match stream.read(&mut buf).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => request.extend_from_slice(&buf[..n]),
                            }
                        }
                        let request = String::from_utf8_lossy(&request).to_string();
                        let if_none_match = request.lines().find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("if-none-match")
                                .then(|| value.trim().to_string())
                        });
                        let response = {
                            let mut served = state.lock().unwrap();
                            served.if_none_match.push(if_none_match.clone());
                            if served.down {
                                b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec()
                            } else if if_none_match.is_some() && if_none_match == served.etag {
                                b"HTTP/1.1 304 Not Modified\r\nconnection: close\r\n\r\n".to_vec()
                            } else {
                                let mut head = format!(
                                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                                    served.body.len()
                                );
                                if let Some(etag) = &served.etag {
                                    head.push_str(&format!("etag: {etag}\r\n"));
                                }
                                if let Some(signature) = &served.signature {
                                    head.push_str(&format!(
                                        "{BUNDLE_SIGNATURE_HEADER}: {signature}\r\n"
                                    ));
                                }
                                head.push_str("\r\n");
                                let mut response = head.into_bytes();
                                response.extend_from_slice(&served.body);
                                response
                            }
                        };
                        let _ = stream.write_all(&response).await;
                        let _ = stream.shutdown().await;
                    });
                }
            });
            Self { url, served }
        }

        /// Change what the registry answers.
        pub(crate) fn serve(&self, change: impl FnOnce(&mut Served)) {
            change(&mut self.served.lock().unwrap());
        }

        /// The If-None-Match value of each request so far.
        pub(crate) fn if_none_match(&self) -> Vec<Option<String>> {
            self.served.lock().unwrap().if_none_match.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_registry::{bundle_data, key_pair, signed, MockRegistry};

    #[tokio::test]
    async fn bundles_signed_with_a_pinned_key_are_accepted() {
        let (pair, public) = key_pair();
        let registry = MockRegistry::start().await;
        let (body, signature) = signed(&pair, &bundle_data("signed"));
        registry.serve(|s| {
            s.body = body;
            s.signature = Some(signature);
        });

        let client = RegistryApiClient::new(registry.url.clone()).with_bundle_keys(vec![public]);
        let Ok(FetchBundleResult::Updated { bundle, .. }) = client.fetch_bundle(None).await else {
            panic!("expected a bundle");
        };
        assert_eq!(
            bundle.signed_by.as_deref(),
            Some(encode_key(&public).as_str())
        );
        assert!(client.trusts_key(bundle.signed_by.as_deref().unwrap()));
        assert_eq!(bundle.servers[0].id, "signed");
    }

    #[tokio::test]
    async fn unsigned_or_mismatched_bundles_are_rejected_once_a_key_is_pinned() {
        let (pair, public) = key_pair();
        let (other, _) = key_pair();
        let data = bundle_data("served");
        let (body, good) = signed(&pair, &data);
        let (_, by_other_key) = signed(&other, &data);
        let (_, of_other_data) = signed(&pair, &bundle_data("something-else"));
        let of_whole_body = base64::engine::general_purpose::STANDARD.encode(pair.sign(&body));
        let registry = MockRegistry::start().await;
        let client = RegistryApiClient::new(registry.url.clone()).with_bundle_keys(vec![public]);

        for (case, signature) in [
            ("unsigned", None),
            ("signed by another key", Some(by_other_key)),
            ("signature of other data", Some(of_other_data)),
            ("signature of the whole body", Some(of_whole_body)),
            ("not base64", Some("%%%".to_string())),
        ] {
            registry.serve(|s| {
                s.body = body.clone();
                s.signature = signature;
            });
            let err = client.fetch_bundle(None).await.expect_err(case);
            assert!(
                err.to_string().contains("registry bundle"),
                "{case}: {err:#}"
            );
        }

        registry.serve(|s| s.signature = Some(good));
        assert!(client.fetch_bundle(None).await.is_ok());
    }

    #[tokio::test]
    async fn without_a_pinned_key_bundles_are_accepted_but_not_marked_verified() {
        let registry = MockRegistry::start().await;
        // The payload can't mark itself verified.
        let data = bundle_data("plain").replacen('{', r#"{"signed_by":"AAAA","#, 1);
        registry.serve(|s| s.body = format!(r#"{{"data":{data}}}"#).into_bytes());

        let client = RegistryApiClient::new(registry.url.clone());
        assert!(!client.verifies_bundles());
        let Ok(FetchBundleResult::Updated { bundle, .. }) = client.fetch_bundle(None).await else {
            panic!("expected a bundle");
        };
        assert_eq!(bundle.signed_by, None);
    }

    #[test]
    fn only_the_official_registry_uses_the_official_keys() {
        // Also checks that every listed key decodes.
        let pinned = !official_bundle_keys().is_empty();
        for url in [
            OFFICIAL_REGISTRY_URL.to_string(),
            format!("{OFFICIAL_REGISTRY_URL}/"),
        ] {
            assert_eq!(RegistryApiClient::new(url).verifies_bundles(), pinned);
        }
        assert!(!RegistryApiClient::new("http://127.0.0.1:9".to_string()).verifies_bundles());

        // The official registry is recognized by host, however it's written.
        for url in [
            "https://api.mcpmux.com",
            "https://API.mcpmux.com/",
            "https://api.mcpmux.com:443",
            "http://api.mcpmux.com",
        ] {
            assert!(is_official_registry(url), "{url}");
        }
        for url in [
            "https://api.mcpmux.com.evil.example",
            "http://127.0.0.1:9",
            "not a url",
        ] {
            assert!(!is_official_registry(url), "{url}");
        }
    }

    #[test]
    fn the_signature_covers_the_data_member_as_served() {
        let (pair, public) = key_pair();
        let data = bundle_data("spaced");
        let (_, signature) = signed(&pair, &data);

        // Whitespace and members around `data` are not part of what is signed.
        let body = format!("{{ \"meta\": {{}},\n  \"data\":  {data} \n}}");
        verify_and_parse(body.as_bytes(), Some(&signature), &[public]).unwrap();

        // The same data serialized differently no longer matches.
        let value: serde_json::Value = serde_json::from_str(&data).unwrap();
        let body = format!(
            r#"{{"data":{}}}"#,
            serde_json::to_string_pretty(&value).unwrap()
        );
        assert!(verify_and_parse(body.as_bytes(), Some(&signature), &[public]).is_err());

        // A second `data` member (one signed, one not) is refused outright.
        let (signed_body, _) = signed(&pair, &data);
        let signed_body = String::from_utf8(signed_body).unwrap();
        let doubled = signed_body.replacen(
            r#"{"data":"#,
            &format!(r#"{{"data":{},"data":"#, bundle_data("unsigned")),
            1,
        );
        assert!(verify_and_parse(doubled.as_bytes(), Some(&signature), &[public]).is_err());
    }

    /// A body without Content-Length is still cut off once it passes the
    /// cap, chunk by chunk, instead of being read to the end.
    #[tokio::test]
    async fn a_body_without_a_length_is_capped_while_reading() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/bundle", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).await;
            // No Content-Length: the body runs until the connection closes.
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n")
                .await;
            for _ in 0..64 {
                if stream.write_all(&[b'x'; 1024]).await.is_err() {
                    break;
                }
            }
            let _ = stream.shutdown().await;
        });

        let response = reqwest::get(&url).await.unwrap();
        assert!(response.content_length().is_none());
        let err = read_capped(response, 4096).await.unwrap_err();
        assert!(err.to_string().contains("larger than 4096 bytes"), "{err}");
    }

    /// Regression test: one server the client can't parse (here an unknown
    /// auth type) must be skipped, not fail the whole bundle.
    #[test]
    fn test_parse_bundle_skips_unparseable_servers() {
        let body = br#"{
            "data": {
                "version": "2.1.0",
                "updated_at": "2026-10-01T00:00:00Z",
                "servers": [
                    {
                        "id": "good",
                        "name": "Good",
                        "transport": { "type": "stdio", "command": "npx" },
                        "auth": { "type": "basic", "instructions": "user/pass" }
                    },
                    {
                        "id": "future",
                        "name": "Future",
                        "transport": { "type": "stdio", "command": "npx" },
                        "auth": { "type": "some_future_auth" }
                    }
                ],
                "categories": [],
                "ui": {
                    "filters": [],
                    "sort_options": [],
                    "default_sort": "name",
                    "items_per_page": 20
                }
            }
        }"#;

        let bundle = parse_bundle(body).expect("bundle should parse");

        let ids: Vec<&str> = bundle.servers.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["good"]);
        assert_eq!(bundle.skipped_servers, 1);

        // The disk cache stores the serialized bundle; the skip count is
        // runtime-only and must not leak into it.
        let cached = serde_json::to_value(&bundle).unwrap();
        assert!(cached.get("skipped_servers").is_none());
        let reloaded: RegistryBundle = serde_json::from_value(cached).unwrap();
        assert_eq!(reloaded.servers.len(), 1);
        assert_eq!(reloaded.skipped_servers, 0);
    }

    #[test]
    fn test_parse_bundle_rejects_malformed_envelope() {
        assert!(parse_bundle(br#"{"data": {"servers": "nope"}}"#).is_err());
    }

    #[tokio::test]
    async fn test_fetch_bundle_from_local() {
        // Uses deployed API by default, or MCPMUX_REGISTRY_URL env var
        let client = RegistryApiClient::new(
            std::env::var("MCPMUX_REGISTRY_URL")
                .unwrap_or_else(|_| "https://api.mcpmux.com".to_string()),
        );

        let result = client.fetch_bundle(None).await;

        // An unreachable registry is tolerated; a bundle we can't parse is not.
        let (bundle, etag) = match result {
            Ok(FetchBundleResult::Updated { bundle, etag }) => (bundle, etag),
            Ok(FetchBundleResult::NotModified) => panic!("no ETag was sent, expected a bundle"),
            Err(e) if e.chain().any(|c| c.is::<serde_json::Error>()) => {
                panic!("registry bundle failed to parse: {:#}", e)
            }
            Err(e) => {
                eprintln!("skipping: registry unreachable: {:#}", e);
                return;
            }
        };

        assert!(
            !bundle.servers.is_empty(),
            "Should have at least one server"
        );
        assert!(!bundle.ui.filters.is_empty(), "Should have filters");
        assert!(
            !bundle.ui.sort_options.is_empty(),
            "Should have sort options"
        );
        assert!(etag.is_some(), "Should have ETag");
    }

    #[tokio::test]
    async fn test_fetch_bundle_with_etag() {
        let client = RegistryApiClient::new(
            std::env::var("MCPMUX_REGISTRY_URL")
                .unwrap_or_else(|_| "https://api.mcpmux.com".to_string()),
        );

        // First fetch to get ETag
        let first_result = client.fetch_bundle(None).await;
        if let Ok(FetchBundleResult::Updated {
            etag: Some(etag), ..
        }) = first_result
        {
            // Second fetch with ETag should return NotModified
            let second_result = client.fetch_bundle(Some(&etag)).await;
            if let Ok(FetchBundleResult::NotModified) = second_result {
                // Success!
            } else {
                // Server might have been updated, that's okay
            }
        }
    }
}
