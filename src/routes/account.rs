//! Customer account center API.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use serde::Deserialize;

use crate::account::{self, limits_from_kyc, trust_from_kyc};
use crate::auth_profile;
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/account", get(account_overview))
        .route("/api/account/profile", post(update_profile))
        .route("/api/account/sessions", get(list_sessions))
        .route("/api/account/sessions/{id}", post(revoke_session))
        .route("/api/account/sessions/revoke-others", post(revoke_others))
        .route("/api/account/events", get(list_events))
        .route("/api/account/data-requests", get(list_requests).post(create_request))
        .with_state(state)
}

#[derive(serde::Serialize)]
struct ErrorBody {
    error: String,
}

fn err(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (status, Json(ErrorBody { error: msg.into() })).into_response()
}

async fn require_session(
    state: &AppState,
    jar: &CookieJar,
    headers: &axum::http::HeaderMap,
) -> Result<(passkey_rs::session::Session, String), axum::response::Response> {
    crate::session_guard::require_bound_session(state, jar, headers).await
}

fn kyc_label(status: &str) -> String {
    match status {
        "draft" => "Черновик".into(),
        "pending" => "На проверке".into(),
        "approved" => "Подтверждена".into(),
        "rejected" => "Отклонена".into(),
        "resubmit" => "Нужна доработка".into(),
        other => other.to_string(),
    }
}

fn event_label(t: &str) -> String {
    match t {
        "login" => "Вход".into(),
        "register" => "Регистрация".into(),
        "logout" => "Выход".into(),
        "session_revoke" => "Сессия завершена".into(),
        "session_revoke_others" => "Завершены другие сессии".into(),
        "profile_update" => "Профиль обновлён".into(),
        "data_request" => "Заявка на обновление данных".into(),
        other => other.to_string(),
    }
}

fn fmt_odt(t: time::OffsetDateTime) -> String {
    t.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| t.to_string())
}

async fn account_overview(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    let (session, current_id) = match require_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    let profile = match auth_profile::get_by_user(&state.auth_db, session.user_id).await {
        Ok(p) => p,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let registered_at = match account::user_registered_at(&state.auth_db, session.user_id).await {
        Ok(t) => t,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let kyc = match crate::kyc::get_or_create_draft(&state.auth_db, session.user_id).await {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let trust = trust_from_kyc(&kyc.status);
    let limits = limits_from_kyc(&kyc.status);
    let profile_complete = profile
        .as_ref()
        .map(auth_profile::profile_complete)
        .unwrap_or(false);

    Json(serde_json::json!({
        "user_id": session.user_id,
        "username": session.username,
        "registered_at": registered_at,
        "profile": profile,
        "profile_complete": profile_complete,
        "kyc": {
            "status": kyc.status,
            "status_label": kyc_label(&kyc.status),
            "note": kyc.reviewer_note,
        },
        "trust": trust,
        "limits": limits,
        "current_session_id": current_id,
        "session": {
            "created_at": fmt_odt(session.created_at),
            "last_seen_at": fmt_odt(session.last_seen_at),
            "expires_at": fmt_odt(session.effective_expires_at()),
            "client_ip": session.client_ip,
            "client_country": session.client_country,
        }
    }))
    .into_response()
}

#[derive(Deserialize)]
struct UpdateProfileBody {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    full_name: Option<String>,
    /// YYYY-MM-DD or empty to clear
    #[serde(default)]
    birth_date: Option<String>,
}

async fn update_profile(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<UpdateProfileBody>,
) -> impl IntoResponse {
    let (session, _) = match require_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Err(r) = crate::session_guard::require_step_up(&session) {
        return r;
    }

    let existing = match auth_profile::get_by_user(&state.auth_db, session.user_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return err(
                StatusCode::BAD_REQUEST,
                "Сначала выберите @ник",
            )
        }
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let display_name = body
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let email = match auth_profile::normalize_email(
        body.email.as_deref().unwrap_or(&existing.email),
    ) {
        Ok(v) => v,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let phone = match auth_profile::normalize_phone(
        body.phone.as_deref().unwrap_or(&existing.phone),
    ) {
        Ok(v) => v,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let full_name = match auth_profile::normalize_full_name(
        body.full_name.as_deref().unwrap_or(&existing.full_name),
    ) {
        Ok(v) => v,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };

    let birth_date = match body.birth_date.as_deref() {
        None => existing.birth_date,
        Some("") => None,
        Some(s) => match chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d") {
            Ok(d) => Some(d),
            Err(_) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "Дата рождения: формат ГГГГ-ММ-ДД",
                )
            }
        },
    };

    let contacts = auth_profile::ProfileContacts {
        display_name,
        email,
        phone,
        full_name,
        birth_date,
    };

    match auth_profile::update_contacts(&state.auth_db, session.user_id, &contacts).await {
        Ok(Some(p)) => {
            let _ = account::record_event(
                &state.auth_db,
                session.user_id,
                "profile_update",
                "",
                "",
                "Обновлены данные профиля",
            )
            .await;
            Json(serde_json::json!({
                "profile": p,
                "complete": auth_profile::profile_complete(&p),
            }))
            .into_response()
        }
        Ok(None) => err(
            StatusCode::BAD_REQUEST,
            "Укажите отображаемое имя (1–64 символа) или сначала выберите @ник",
        ),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn list_sessions(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    let (session, current_id) = match require_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match state.passkey.sessions.list_for_user(session.user_id).await {
        Ok(rows) => {
            let list: Vec<_> = rows
                .into_iter()
                .map(|s| {
                    serde_json::json!({
                        "id": s.id,
                        "current": s.id == current_id,
                        "created_at": fmt_odt(s.created_at),
                        "last_seen_at": fmt_odt(s.last_seen_at),
                        "expires_at": fmt_odt(s.effective_expires_at()),
                        "bound": s.client_binding.is_some(),
                        "client_ip": s.client_ip,
                        "client_country": s.client_country,
                    })
                })
                .collect();
            Json(serde_json::json!({ "sessions": list })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn revoke_session(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let (session, current_id) = match require_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Err(r) = crate::session_guard::require_step_up(&session) {
        return r;
    }
    if id == current_id {
        return err(
            StatusCode::BAD_REQUEST,
            "Текущую сессию завершите через «Выйти»",
        );
    }
    let owned = match state.passkey.sessions.list_for_user(session.user_id).await {
        Ok(rows) => rows.iter().any(|s| s.id == id),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if !owned {
        return err(StatusCode::NOT_FOUND, "Сессия не найдена");
    }
    if let Err(e) = state.passkey.sessions.destroy(&id).await {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    Json(serde_json::json!({ "ok": true })).into_response()
}

async fn revoke_others(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    let (session, current_id) = match require_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Err(r) = crate::session_guard::require_step_up(&session) {
        return r;
    }
    match state
        .passkey
        .sessions
        .destroy_all_for_user(session.user_id, Some(&current_id))
        .await
    {
        Ok(n) => Json(serde_json::json!({ "revoked": n })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

async fn list_events(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(q): Query<EventsQuery>,
) -> impl IntoResponse {
    let (session, _) = match require_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match account::list_events(&state.auth_db, session.user_id, q.limit).await {
        Ok(rows) => {
            let list: Vec<_> = rows
                .into_iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id,
                        "event_type": e.event_type,
                        "event_label": event_label(&e.event_type),
                        "ip_hint": e.ip_hint,
                        "detail": e.detail,
                        "created_at": e.created_at,
                    })
                })
                .collect();
            Json(serde_json::json!({ "events": list })).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct DataRequestBody {
    request_type: String,
    payload: String,
}

async fn list_requests(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    let (session, _) = match require_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    match account::list_data_requests(&state.auth_db, session.user_id).await {
        Ok(rows) => Json(serde_json::json!({ "requests": rows })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn create_request(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<DataRequestBody>,
) -> impl IntoResponse {
    let (session, _) = match require_session(&state, &jar, &headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let allowed = ["personal_data", "address", "contacts", "other"];
    if !allowed.contains(&body.request_type.as_str()) {
        return err(StatusCode::BAD_REQUEST, "Неверный тип заявки");
    }
    let payload = body.payload.trim();
    if payload.is_empty() || payload.len() > 4000 {
        return err(
            StatusCode::BAD_REQUEST,
            "Опишите изменения (до 4000 символов)",
        );
    }
    match account::create_data_request(
        &state.auth_db,
        session.user_id,
        &body.request_type,
        payload,
    )
    .await
    {
        Ok(r) => {
            let _ = account::record_event(
                &state.auth_db,
                session.user_id,
                "data_request",
                "",
                "",
                &format!("Тип: {}", body.request_type),
            )
            .await;
            Json(r).into_response()
        }
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}