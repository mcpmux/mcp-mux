//! CIMD (Client ID Metadata Document) fetcher
//!
//! Handles HTTP fetching of client metadata from URLs per the OAuth Client ID
//! Metadata Document specification (draft).
//!
//! The client_id URL comes from whoever calls `/authorize`, so the fetch is
//! limited to what a metadata document needs: an `https` URL on a public
//! address, no redirects, a short timeout and a small body.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Largest metadata document accepted, in bytes.
const MAX_DOCUMENT_BYTES: usize = 16 * 1024;

/// Client metadata from CIMD document
#[derive(Debug, Clone, Deserialize)]
pub struct CimdMetadata {
    pub client_id: String,
    pub client_name: String,
    pub logo_uri: Option<String>,
    pub client_uri: Option<String>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub redirect_uris: Vec<String>,
    pub grant_types: Option<Vec<String>>,
    pub response_types: Option<Vec<String>>,
    pub token_endpoint_auth_method: Option<String>,
    pub scope: Option<String>,
}

/// Fetches client metadata from CIMD URLs
///
/// Single responsibility: HTTP operations only, no persistence
pub struct CimdMetadataFetcher {
    http_client: reqwest::Client,
    allow_loopback_http: bool,
}

impl CimdMetadataFetcher {
    /// Create a CIMD fetcher that only fetches `https` URLs on public addresses.
    pub fn new() -> Result<Self> {
        let http_client = Self::client_builder()
            .dns_resolver(Arc::new(PublicAddressResolver))
            .build()?;
        Ok(Self {
            http_client,
            allow_loopback_http: false,
        })
    }

    /// For tests only: also accept `http://` documents served from loopback
    /// (e.g. a local mock server). Never use this for a real gateway.
    pub fn allowing_loopback_http_for_tests() -> Result<Self> {
        Ok(Self {
            http_client: Self::client_builder().build()?,
            allow_loopback_http: true,
        })
    }

    fn client_builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
    }

    /// Fetch metadata from a CIMD URL
    ///
    /// Returns the parsed metadata or an error if fetching fails
    pub async fn fetch(&self, client_id_url: &str) -> Result<CimdMetadata> {
        let url = check_client_id_url(client_id_url, self.allow_loopback_http)?;
        info!("[CIMD] Fetching client metadata from: {}", client_id_url);

        let mut response = self
            .http_client
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            bail!("Failed to fetch CIMD metadata: HTTP {}", response.status());
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_DOCUMENT_BYTES as u64)
        {
            bail!("CIMD metadata document is too large");
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > MAX_DOCUMENT_BYTES {
                bail!("CIMD metadata document is too large");
            }
            body.extend_from_slice(&chunk);
        }
        let metadata: CimdMetadata =
            serde_json::from_slice(&body).context("CIMD metadata is not valid JSON")?;

        // Validate that client_id in metadata matches the URL
        if metadata.client_id != client_id_url {
            bail!(
                "CIMD client_id mismatch: URL='{}', metadata.client_id='{}'",
                client_id_url,
                metadata.client_id
            );
        }

        info!(
            "[CIMD] Successfully fetched metadata for: {}",
            client_id_url
        );
        Ok(metadata)
    }

    /// Check if a string looks like a CIMD URL
    pub fn is_cimd_url(client_id: &str) -> bool {
        client_id.starts_with("https://") || client_id.starts_with("http://")
    }
}

impl Default for CimdMetadataFetcher {
    fn default() -> Self {
        Self::new().expect("Failed to create default CIMD fetcher")
    }
}

/// Check a client_id URL before fetching it: `https` with a path, no
/// credentials or fragment, and not an IP literal outside the public internet.
/// Host names are checked when they resolve (see [`PublicAddressResolver`]).
fn check_client_id_url(client_id_url: &str, allow_loopback_http: bool) -> Result<url::Url> {
    let url = url::Url::parse(client_id_url).context("CIMD client_id is not a valid URL")?;
    let host = url.host().context("CIMD client_id has no host")?;
    let is_loopback = match &host {
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
        url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
    };
    let test_loopback = allow_loopback_http && is_loopback;

    if url.scheme() != "https" && !(test_loopback && url.scheme() == "http") {
        bail!("CIMD client_id must be an https URL");
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        bail!("CIMD client_id must not contain credentials or a fragment");
    }
    if url.path().is_empty() || url.path() == "/" {
        bail!("CIMD client_id must include a path");
    }
    let literal_ip = match host {
        url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
        url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
        url::Host::Domain(_) => None,
    };
    if let Some(ip) = literal_ip {
        if !test_loopback && !is_public_address(ip) {
            bail!("CIMD client_id must not point at a local or private address");
        }
    }
    Ok(url)
}

/// Whether `ip` is a globally routable unicast address.
fn is_public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_ipv4(v4),
            None => is_public_ipv6(v6),
        },
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..=127).contains(&b)) // shared address space (CGNAT)
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 198 && (18..=19).contains(&b)) // benchmarking
        || a >= 240) // reserved
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (first & 0xfe00) == 0xfc00 // unique local
        || (first & 0xffc0) == 0xfe80 // link-local
        || first == 0x2001 && ip.segments()[1] == 0x0db8 // documentation
        || ip.segments()[..6] == [0, 0, 0, 0, 0, 0]) // IPv4-compatible / reserved
}

/// DNS resolver that only returns public addresses, so a host name can't be
/// used to reach loopback, private or link-local services.
struct PublicAddressResolver;

impl reqwest::dns::Resolve for PublicAddressResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|addr| is_public_address(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} does not resolve to a public address").into());
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_cimd_url() {
        assert!(CimdMetadataFetcher::is_cimd_url(
            "https://example.com/client.json"
        ));
        assert!(CimdMetadataFetcher::is_cimd_url(
            "http://localhost:3000/client"
        ));
        assert!(!CimdMetadataFetcher::is_cimd_url("mcp_abc123"));
        assert!(!CimdMetadataFetcher::is_cimd_url("client-name"));
    }

    #[test]
    fn client_id_urls_must_be_https_on_public_hosts() {
        assert!(check_client_id_url("https://example.com/client.json", false).is_ok());

        for url in [
            "http://example.com/client.json",
            "https://example.com",
            "https://user:pw@example.com/client.json",
            "https://example.com/client.json#x",
            "https://127.0.0.1/client.json",
            "https://10.0.0.5/client.json",
            "https://169.254.169.254/latest/meta-data/",
            "https://[::1]/client.json",
            "https://[fd00::1]/client.json",
            "https://[::ffff:192.168.1.1]/client.json",
            "file:///etc/passwd",
        ] {
            assert!(
                check_client_id_url(url, false).is_err(),
                "{url} must be refused"
            );
        }
    }

    #[test]
    fn loopback_http_is_only_allowed_in_test_mode() {
        let url = "http://127.0.0.1:8080/client.json";
        assert!(check_client_id_url(url, false).is_err());
        assert!(check_client_id_url(url, true).is_ok());
        // Test mode still refuses non-loopback plain http.
        assert!(check_client_id_url("http://example.com/client.json", true).is_err());
    }

    #[test]
    fn public_address_classification() {
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public_address(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fe80::1",
            "fc00::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(!is_public_address(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn host_names_that_resolve_locally_are_not_fetched() {
        let fetcher = CimdMetadataFetcher::new().unwrap();
        let err = fetcher
            .fetch("https://localhost:9/client.json")
            .await
            .expect_err("localhost must not be fetched");
        assert!(
            format!("{err:#}").contains("public address"),
            "unexpected error: {err:#}"
        );
    }
}
