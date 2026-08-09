//! Tunnel RP rewrite for cloudflared Quick Tunnel.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use passkey_rs::RelyingParty;
use serde::Deserialize;

use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/debug/rp", post(set_rp))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct SetRpBody {
    origin: String,
    #[serde(default)]
    rp_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

async fn set_rp(State(state): State<AppState>, Json(body): Json<SetRpBody>) -> impl IntoResponse {
    if !matches!(
        std::env::var("ALLOW_DEBUG_RP").as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    ) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "ALLOW_DEBUG_RP disabled" })),
        )
            .into_response();
    }

    let origin = body.origin.trim_end_matches('/').to_string();
    let url = match url::Url::parse(&origin) {
        Ok(u) => u,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("bad origin: {e}") })),
            )
                .into_response();
        }
    };
    let host = match url.host_str() {
        Some(h) => h.to_string(),
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "origin without host" })),
            )
                .into_response();
        }
    };
    let rp_id = body.rp_id.unwrap_or(host);
    let name = body
        .name
        .unwrap_or_else(|| state.passkey.relying_party().name);
    let rp = RelyingParty::new(rp_id, name, origin.clone());
    state.passkey.set_relying_party(rp.clone());
    if let Err(e) = crate::rp_persist::save_rp(&state.auth_db, &rp).await {
        tracing::warn!(error = %e, "failed to persist relying party");
    }
    tracing::info!(%origin, "debug: relying party updated from tunnel");
    Json(serde_json::json!({ "ok": true, "origin": origin })).into_response()
}
