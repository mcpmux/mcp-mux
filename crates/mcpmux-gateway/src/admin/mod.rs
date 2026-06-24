//! Web admin HTTP server (REST + static SPA).
//!
//! Serves the built React admin UI and `/api/v1/*` REST endpoints on a
//! separate loopback port (default `45819`), gated by Cloudflare Access
//! when configured.

pub mod bridge_context;
pub mod command_bridge;
mod config;
pub mod event_hub;
mod handlers;
mod live_runtime;
mod middleware;
mod router;
pub mod runtime;
mod server;
pub mod ui_events;
pub mod write_runtime;

pub use bridge_context::{AdminBridgeCtx, BackendBuildStamp};
pub use config::{AdminConfig, CF_ACCESS_JWT_HEADER, DEFAULT_ADMIN_PORT};
pub use event_hub::AdminEventHub;
#[cfg(any(test, feature = "test-utils"))]
pub use handlers::error::format_bridge_error_message;
pub use live_runtime::LiveGatewayRuntime;
pub use middleware::{new_csrf_token_store, CfAccessError, CfAccessValidator, CSRF_HEADER};
pub use router::{build_admin_router, AdminState};
pub use runtime::GatewayRuntime;
#[cfg(any(test, feature = "test-utils"))]
pub use runtime::StubGatewayRuntime;
pub use server::{AdminServer, AdminServerHandle};
pub use ui_events::{map_domain_event_to_ui, AdminUiEventBus, UiEvent};
pub use write_runtime::GatewayWriteRuntime;
pub use write_runtime::LiveGatewayWriteRuntime;
#[cfg(any(test, feature = "test-utils"))]
pub use write_runtime::StubGatewayWriteRuntime;

#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub use middleware::{test_valid_jwt, test_validator};
