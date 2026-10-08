//! OAuth for the gateway's own authorization server: dynamic client
//! registration and redirect-URI policy. Outbound OAuth to MCP servers lives
//! in `pool::oauth` (on top of rmcp).

mod dcr;

pub use dcr::{
    filter_valid_redirect_uris, is_redirect_uri_allowed, is_valid_registered_redirect_uri,
    process_dcr_request, validate_redirect_uris, DcrError, DcrRequest, DcrResponse,
};
