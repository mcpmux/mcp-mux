//! Admin server configuration.

use std::sync::Arc;

use super::middleware::CfAccessValidator;

/// Default admin listen port (loopback + CF tunnel).
pub const DEFAULT_ADMIN_PORT: u16 = 45819;

/// Cloudflare Access JWT header forwarded by the tunnel edge.
pub const CF_ACCESS_JWT_HEADER: &str = "CF-Access-Jwt-Assertion";

/// Admin HTTP server configuration.
#[derive(Clone)]
pub struct AdminConfig {
    /// Host to bind to (default loopback).
    pub host: String,
    /// Port to listen on.
    pub port: u16,
    /// Require and validate `CF-Access-Jwt-Assertion` when true.
    ///
    /// When enabled, **all** routes including `/api/v1/health` require a valid JWT,
    /// or matching `CF-Access-Client-Id` / `CF-Access-Client-Secret` service-token
    /// headers when `MCPMUX_CF_ACCESS_CLIENT_ID` and `MCPMUX_CF_ACCESS_CLIENT_SECRET`
    /// are set in the admin process environment.
    /// Cloudflare Tunnel origin health probes do not send `CF-Access-Jwt-Assertion`;
    /// do not rely on tunnel health checks against the admin origin — use an external
    /// monitor or a separate unauthenticated probe path if needed.
    pub trust_cf_access: bool,
    /// Cloudflare team domain for JWT cert validation (e.g. `myteam`).
    pub cf_team_domain: Option<String>,
    /// Optional CF Access application AUD tag.
    pub cf_access_audience: Option<String>,
    /// Inject a validator (integration tests); skips cert fetch when set.
    pub cf_validator_override: Option<Arc<CfAccessValidator>>,
}

impl std::fmt::Debug for AdminConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("trust_cf_access", &self.trust_cf_access)
            .field("cf_team_domain", &self.cf_team_domain)
            .field("cf_access_audience", &self.cf_access_audience)
            .field(
                "cf_validator_override",
                &self.cf_validator_override.is_some(),
            )
            .finish()
    }
}

impl Default for AdminConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: DEFAULT_ADMIN_PORT,
            trust_cf_access: false,
            cf_team_domain: None,
            cf_access_audience: None,
            cf_validator_override: None,
        }
    }
}

impl AdminConfig {
    /// Socket address string for binding.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
