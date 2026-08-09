//! Bound session checks for customer APIs (network /24 + UA + fingerprint).

use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use axum_extra::extract::CookieJar;
use passkey_rs::session::{
    hash_fingerprint, hash_user_agent, network_binding_key, BindingFail, Session, SessionEvidence,
    FINGERPRINT_HEADER,
};

use crate::AppState;

#[derive(serde::Serialize)]
struct ErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}

fn err(status: StatusCode, msg: impl Into<String>, code: Option<&'static str>) -> axum::response::Response {
    (
        status,
        Json(ErrorBody {
            error: msg.into(),
            code,
        }),
    )
        .into_response()
}

fn client_ip(headers: &HeaderMap, trust_forwarded: bool) -> String {
    if trust_forwarded {
        for key in ["cf-connecting-ip", "true-client-ip", "x-real-ip"] {
            if let Some(v) = headers.get(key).and_then(|v| v.to_str().ok()) {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
        if let Some(xff) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
        {
            if let Some(first) = xff.split(',').next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    "direct".into()
}

/// Build evidence from the inbound request (after Envoy / secure-rpc header forward).
pub fn evidence_from_headers(headers: &HeaderMap, trust_forwarded: bool) -> SessionEvidence {
    let ip = client_ip(headers, trust_forwarded);
    let ua = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let fp = headers
        .get(FINGERPRINT_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    SessionEvidence {
        network_binding: network_binding_key(&ip),
        ua_hash: hash_user_agent(ua),
        fingerprint_hash: hash_fingerprint(fp),
        client_ip: Some(ip).filter(|s| s != "direct"),
        client_country: None,
    }
}

/// Cookie session + proof + anti-theft binding. On mismatch the session is destroyed.
pub async fn require_bound_session(
    state: &AppState,
    jar: &CookieJar,
    headers: &HeaderMap,
) -> Result<(Session, String), axum::response::Response> {
    let cookie_name = &state.passkey.config.cookies.session_name;
    let Some(cookie) = jar.get(cookie_name) else {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Войдите в аккаунт",
            Some("no_session"),
        ));
    };
    let current_id = cookie.value().to_string();
    let Some(session) = state
        .passkey
        .sessions
        .get(&current_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string(), None))?
    else {
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Сессия истекла — войдите снова",
            Some("session_expired"),
        ));
    };

    let proof = jar
        .get(&state.passkey.config.cookies.proof_name)
        .map(|c| c.value().to_string())
        .unwrap_or_default();
    if !session.proof_matches(&proof) {
        tracing::warn!(user_id = %session.user_id, "destroying session — proof cookie mismatch");
        let _ = state.passkey.sessions.destroy(&current_id).await;
        return Err(err(
            StatusCode::UNAUTHORIZED,
            "Сессия повреждена — войдите снова",
            Some("session_proof_mismatch"),
        ));
    }

    if state.passkey.config.bind_session_to_client {
        let evidence = evidence_from_headers(headers, state.passkey.config.trust_forwarded_headers);
        if let Err(fail) = session.verify_evidence(&evidence) {
            tracing::warn!(?fail, user_id = %session.user_id, "destroying bound session");
            let _ = state.passkey.sessions.destroy(&current_id).await;
            let (msg, code) = match fail {
                BindingFail::Network => (
                    "Сеть сменилась — войдите снова",
                    "session_network_changed",
                ),
                BindingFail::UserAgent => (
                    "Клиент изменился — войдите снова",
                    "session_ua_changed",
                ),
                BindingFail::Fingerprint => (
                    "Устройство не совпало — войдите снова",
                    "session_fingerprint_changed",
                ),
            };
            return Err(err(StatusCode::UNAUTHORIZED, msg, Some(code)));
        }
    }

    Ok((session, current_id))
}

/// Fresh passkey assertion required for money moves / profile changes / long statements.
pub fn require_step_up(session: &Session) -> Result<(), axum::response::Response> {
    let now = time::OffsetDateTime::now_utc();
    if session.has_fresh_step_up(now) {
        return Ok(());
    }
    Err(err(
        StatusCode::FORBIDDEN,
        "Подтвердите действие passkey",
        Some("step_up_required"),
    ))
}
