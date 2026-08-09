//! Reject localhost Host when the demo is meant to be tunnel-only.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Returns 403 for `Host: localhost` / loopback when `BLOCK_LOCALHOST` is on.
pub async fn block_localhost_host(req: Request, next: Next) -> Response {
    if !matches!(
        std::env::var("BLOCK_LOCALHOST").as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    ) {
        return next.run(req).await;
    }

    // Container health probes use Host: localhost — allow them through.
    if req.uri().path() == "/healthz" {
        return next.run(req).await;
    }

    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let host_only = host.split(':').next().unwrap_or("");

    if host_only == "localhost"
        || host_only == "127.0.0.1"
        || host_only == "[::1]"
        || host_only == "::1"
    {
        return (
            StatusCode::FORBIDDEN,
            "localhost disabled — use the Cloudflare tunnel HTTPS URL",
        )
            .into_response();
    }

    next.run(req).await
}
