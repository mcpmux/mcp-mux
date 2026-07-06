//! Embedded desktop React UI, served headless at `/app`.
//!
//! The same `apps/desktop` React app the Tauri shell runs is built for the web
//! (`pnpm build:web`, base `/app/`), embedded here with `rust-embed`, and
//! served by the gateway. In the browser it runs its HTTP transport, driving
//! the command-mirror RPC (`/admin/api/rpc/<command>`) — so the UI is
//! identical to desktop, only the transport differs.
//!
//! Feature-gated (`embed-ui`) so a normal `cargo build --workspace` never
//! requires a prebuilt frontend.

use axum::{
    extract::Path,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::RustEmbed;

/// The Vite build output. Path is relative to this crate's manifest dir.
#[derive(RustEmbed)]
#[folder = "../../apps/desktop/dist"]
struct WebAssets;

fn serve_asset(path: &str) -> Response {
    match WebAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Serve `/app` and `/app/` → index.html; `/app/*path` → the asset, falling
/// back to index.html for client-side routes (SPA).
async fn app_index() -> Response {
    serve_asset("index.html")
}

async fn app_path(Path(path): Path<String>) -> Response {
    // Vite emits assets under `assets/…`; index.html references `/app/assets/…`.
    // Requests arrive here without the `/app/` prefix (nest strips it).
    if WebAssets::get(&path).is_some() {
        serve_asset(&path)
    } else {
        // Unknown path under /app → SPA entry point.
        serve_asset("index.html")
    }
}

/// Router mounting the embedded web admin under `/app`.
pub fn router() -> Router {
    Router::new()
        .route("/app", get(app_index))
        .route("/app/", get(app_index))
        .route("/app/{*path}", get(app_path))
        // `/` → the app.
        .route("/", get(redirect_to_app))
}

async fn redirect_to_app(_uri: Uri) -> Response {
    (
        StatusCode::TEMPORARY_REDIRECT,
        [(header::LOCATION, "/app/")],
    )
        .into_response()
}
