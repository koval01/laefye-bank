//! Admin console API — gated by ADMIN_HANDLES.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth_profile;
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/admin/me", get(admin_me))
        .route("/api/admin/fx/settings", get(get_fx).post(set_fx))
        .route("/api/admin/fx/refresh", post(refresh_cbr))
        .route("/api/admin/fx/quotes", get(list_quotes))
        .route("/api/admin/kyc/cases", get(list_kyc))
        .route("/api/admin/kyc/cases/{id}", post(review_kyc))
        .route("/api/admin/kyc/documents/{id}", get(download_doc))
        .route("/api/admin/directory", get(list_profiles))
        .route("/api/admin/crypto/settings", get(get_crypto).post(set_crypto))
        .route("/api/admin/crypto/simulate-deposit", post(simulate_deposit))
        .with_state(state)
}

fn err(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (status, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}

async fn require_admin(
    state: &AppState,
    jar: &CookieJar,
) -> Result<(passkey_rs::Session, auth_profile::BankProfile), axum::response::Response> {
    let cookie_name = &state.passkey.config.cookies.session_name;
    let Some(cookie) = jar.get(cookie_name) else {
        return Err(err(StatusCode::UNAUTHORIZED, "no session"));
    };
    let Some(session) = state
        .passkey
        .sessions
        .get(cookie.value())
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    else {
        return Err(err(StatusCode::UNAUTHORIZED, "session expired"));
    };
    let profile = auth_profile::get_by_user(&state.auth_db, session.user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| err(StatusCode::FORBIDDEN, "claim a handle first"))?;
    if !state
        .admin_handles
        .iter()
        .any(|h| h == &profile.handle)
    {
        return Err(err(StatusCode::FORBIDDEN, "admin only"));
    }
    Ok((session, profile))
}

async fn admin_me(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    match require_admin(&state, &jar).await {
        Ok((session, profile)) => Json(serde_json::json!({
            "ok": true,
            "user_id": session.user_id,
            "handle": profile.handle,
            "kyc_enabled": state.kyc.is_some(),
        }))
        .into_response(),
        Err(r) => r,
    }
}

async fn get_fx(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Err(r) = require_admin(&state, &jar).await {
        return r;
    }
    match crate::fx::load_settings(&state.ledger_db).await {
        Ok(s) => Json(s).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct FxBody {
    margin_bps: i32,
    spread_bps: i32,
    enabled_currencies: Vec<String>,
}

async fn set_fx(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<FxBody>,
) -> impl IntoResponse {
    let (session, _) = match require_admin(&state, &jar).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match crate::fx::update_settings(
        &state.ledger_db,
        body.margin_bps,
        body.spread_bps,
        body.enabled_currencies,
        Some(session.user_id),
    )
    .await
    {
        Ok(s) => Json(s).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn refresh_cbr(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Err(r) = require_admin(&state, &jar).await {
        return r;
    }
    match crate::fx::refresh_cbr(&state.ledger_db, &state.cbr_url).await {
        Ok(n) => Json(serde_json::json!({ "ok": true, "rates": n })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, e.to_string()),
    }
}

async fn list_quotes(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Err(r) = require_admin(&state, &jar).await {
        return r;
    }
    let settings = match crate::fx::load_settings(&state.ledger_db).await {
        Ok(s) => s,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let mut out = Vec::new();
    for base in &settings.enabled_currencies {
        if base == "RUB" {
            continue;
        }
        match crate::fx::quote_pair(&state.ledger_db, base, "RUB").await {
            Ok(q) => out.push(q),
            Err(_) => continue,
        }
    }
    Json(out).into_response()
}

async fn list_kyc(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Err(r) = require_admin(&state, &jar).await {
        return r;
    }
    let rows = sqlx::query_as::<_, (Uuid, Uuid, String, String, chrono::DateTime<chrono::Utc>)>(
        r#"
        SELECT id, user_id, status, reviewer_note, created_at
          FROM kyc_cases
         ORDER BY created_at DESC
         LIMIT 100
        "#,
    )
    .fetch_all(&state.auth_db)
    .await;
    match rows {
        Ok(list) => {
            let json: Vec<_> = list
                .into_iter()
                .map(|(id, user_id, status, reviewer_note, created_at)| {
                    serde_json::json!({
                        "id": id,
                        "user_id": user_id,
                        "status": status,
                        "reviewer_note": reviewer_note,
                        "created_at": created_at,
                    })
                })
                .collect();
            Json(json).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct ReviewBody {
    status: String,
    #[serde(default)]
    note: String,
}

async fn review_kyc(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
    Json(body): Json<ReviewBody>,
) -> impl IntoResponse {
    let (session, _) = match require_admin(&state, &jar).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match crate::kyc::set_status(
        &state.auth_db,
        id,
        &body.status,
        &body.note,
        session.user_id,
    )
    .await
    {
        Ok(c) => Json(c).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn download_doc(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let (session, _) = match require_admin(&state, &jar).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let Some(store) = &state.kyc else {
        return err(StatusCode::SERVICE_UNAVAILABLE, "KYC store offline");
    };
    let row: Option<(String, String, Uuid)> = sqlx::query_as(
        "SELECT object_key, content_type, case_id FROM kyc_documents WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.auth_db)
    .await
    .unwrap_or(None);
    let Some((object_key, content_type, case_id)) = row else {
        return err(StatusCode::NOT_FOUND, "document not found");
    };
    let _ = crate::kyc::audit(
        &state.auth_db,
        Some(session.user_id),
        "document.download",
        Some(case_id),
        Some(id),
        "admin",
    )
    .await;
    match store.get_decrypted(&object_key).await {
        Ok(bytes) => (
            [
                (axum::http::header::CONTENT_TYPE, content_type),
                (
                    axum::http::header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"kyc-{id}\""),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn list_profiles(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Err(r) = require_admin(&state, &jar).await {
        return r;
    }
    let rows = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT user_id, handle, display_name FROM bank_profiles ORDER BY handle LIMIT 500",
    )
    .fetch_all(&state.auth_db)
    .await;
    match rows {
        Ok(list) => Json(
            list.into_iter()
                .map(|(user_id, handle, display_name)| {
                    serde_json::json!({ "user_id": user_id, "handle": handle, "display_name": display_name })
                })
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_crypto(State(state): State<AppState>, jar: CookieJar) -> impl IntoResponse {
    if let Err(r) = require_admin(&state, &jar).await {
        return r;
    }
    match crate::crypto::load_settings(&state.ledger_db).await {
        Ok(s) => Json(serde_json::json!({
            "settings": s,
            "mode": format!("{:?}", state.crypto.mode).to_ascii_lowercase(),
            "seed_configured": state.crypto.master_seed.is_some(),
        }))
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct CryptoBody {
    usdt_usd_mid: rust_decimal::Decimal,
    deposit_spread_bps: i32,
    withdraw_spread_bps: i32,
    min_deposit_usdt_minor: i64,
    min_withdraw_usdt_minor: i64,
    enabled: bool,
}

async fn set_crypto(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<CryptoBody>,
) -> impl IntoResponse {
    let (session, _) = match require_admin(&state, &jar).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match crate::crypto::update_settings(
        &state.ledger_db,
        body.usdt_usd_mid,
        body.deposit_spread_bps,
        body.withdraw_spread_bps,
        body.min_deposit_usdt_minor,
        body.min_withdraw_usdt_minor,
        body.enabled,
        Some(session.user_id),
    )
    .await
    {
        Ok(s) => Json(s).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

#[derive(Deserialize)]
struct SimulateDepositBody {
    user_id: Uuid,
    usdt_minor: i64,
    #[serde(default)]
    tx_hash: Option<String>,
}

/// Ops helper: credit a deposit without waiting for the chain watcher (also used in mock mode).
async fn simulate_deposit(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<SimulateDepositBody>,
) -> impl IntoResponse {
    if let Err(r) = require_admin(&state, &jar).await {
        return r;
    }
    let Some(seed) = state.crypto.master_seed else {
        return err(StatusCode::SERVICE_UNAVAILABLE, "TRON_MASTER_SEED required");
    };
    let addr = match crate::crypto::ensure_deposit_address(&state.ledger_db, body.user_id, &seed)
        .await
    {
        Ok(a) => a,
        Err(e) => return err(StatusCode::BAD_REQUEST, e.to_string()),
    };
    let tx_hash = body
        .tx_hash
        .unwrap_or_else(|| format!("sim-{}", Uuid::new_v4().simple()));
    match crate::crypto::credit_deposit(
        &state.ledger_db,
        body.user_id,
        &tx_hash,
        Some("TSimulatedSender"),
        &addr.address,
        body.usdt_minor,
        Some(chrono::Utc::now()),
    )
    .await
    {
        Ok((deposit_id, credit, fee, replay)) => Json(serde_json::json!({
            "deposit_id": deposit_id,
            "rub_credit_minor": credit,
            "fee_minor": fee,
            "already_credited": replay,
            "tx_hash": tx_hash,
            "address": addr.address,
        }))
        .into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}
