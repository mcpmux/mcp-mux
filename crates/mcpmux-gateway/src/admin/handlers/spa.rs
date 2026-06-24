//! Static SPA fallback when the production build is missing.

use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};

const MISSING_SPA_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>McpMux Web Admin — build required</title>
  <style>
    body { font-family: system-ui, sans-serif; max-width: 40rem; margin: 3rem auto; padding: 0 1rem; line-height: 1.5; }
    code { background: #f4f4f5; padding: 0.15rem 0.35rem; border-radius: 0.25rem; }
  </style>
</head>
<body>
  <h1>Web admin UI not built</h1>
  <p>The admin HTTP server is running, but <code>index.html</code> was not found in the configured frontend dist directory.</p>
  <p>From the repo root, run:</p>
  <pre><code>pnpm build:web:admin</code></pre>
  <p>Then restart web admin mode in McpMux Settings (or restart the desktop app).</p>
</body>
</html>"#;

/// Fallback page when the SPA build is absent.
pub async fn missing_spa_build() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(MISSING_SPA_HTML),
    )
        .into_response()
}
