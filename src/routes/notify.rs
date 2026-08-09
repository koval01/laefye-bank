//! Session heartbeat + in-app notifications API.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use uuid::Uuid;

use crate::notifications;
use crate::session_guard::require_bound_session;
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/notifications", get(list_notifications))
        .route("/api/notifications/read-all", post(read_all))
        .route("/api/notifications/{id}/read", post(read_one))
        .with_state(state)
}

/// Clear-HTTPS heartbeat so rotating proof cookies can be Set-Cookie'd.
pub fn heartbeat_router(state: AppState) -> Router {
    Router::new()
        .route("/api/session/heartbeat", get(heartbeat))
        .with_state(state)
}

#[derive(serde::Serialize)]
struct ErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}

fn err(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(ErrorBody {
            error: msg.into(),
            code: None,
        }),
    )
        .into_response()
}

async fn heartbeat(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (session, session_id) = match require_bound_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let presented = jar
        .get(&state.passkey.config.cookies.proof_name)
        .map(|c| c.value().to_string())
        .unwrap_or_default();
    let new_proof = match state
        .passkey
        .sessions
        .rotate_proof(&session_id, &presented)
        .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            // Race with concurrent activity — do not kill the browser session.
            return err(
                StatusCode::SERVICE_UNAVAILABLE,
                "Сессия обновляется — повторите через мгновение",
            );
        }
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let unread = notifications::unread_count(&state.auth_db, session.user_id)
        .await
        .unwrap_or(0);
    let jar = passkey_rs::axum::cookies::set_session_pair(
        jar,
        &state.passkey.config.cookies,
        &session_id,
        &new_proof,
    );
    (
        jar,
        Json(serde_json::json!({
            "ok": true,
            "server_time": chrono::Utc::now().to_rfc3339(),
            "session_expires_at": session.effective_expires_at()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
            "unread": unread,
            "idle_secs": session.idle_secs,
            "proof_rotated": true,
        })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct ListQ {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    30
}

async fn list_notifications(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(q): Query<ListQ>,
) -> impl IntoResponse {
    let (session, _) = match require_bound_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match notifications::list(&state.auth_db, session.user_id, q.limit).await {
        Ok(items) => {
            let unread = items.iter().filter(|n| n.read_at.is_none()).count();
            Json(serde_json::json!({ "notifications": items, "unread": unread })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn read_one(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let (session, _) = match require_bound_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match notifications::mark_read(&state.auth_db, session.user_id, id).await {
        Ok(_) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn read_all(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (session, _) = match require_bound_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match notifications::mark_all_read(&state.auth_db, session.user_id).await {
        Ok(n) => Json(serde_json::json!({ "ok": true, "marked": n })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
