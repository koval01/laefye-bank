//! Bank API: profile, accounts, transfers, directory.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth_profile;
use crate::documents::{self, PartyInfo};
use crate::ledger::{self, TransferRequest};
use crate::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/bank/me", get(bank_me))
        .route("/api/bank/profile", post(claim_profile))
        .route("/api/accounts", get(list_accounts))
        .route("/api/accounts/open", post(open_account))
        .route(
            "/api/accounts/{id}/transactions",
            get(account_transactions),
        )
        .route("/api/transactions/{entry_id}", get(transaction_detail))
        .route(
            "/api/documents/statement",
            get(document_statement),
        )
        .route(
            "/api/documents/receipt/{entry_id}",
            get(document_receipt),
        )
        .route("/api/transfers", post(create_transfer))
        .route("/api/fx/quote", get(fx_quote))
        .route("/api/fx/convert", post(fx_convert))
        .route("/api/market", get(market_snapshot))
        .route("/api/directory/lookup", get(lookup_handle))
        .route("/api/kyc", get(kyc_status).post(kyc_submit))
        .route("/api/kyc/documents", post(kyc_upload))
        .route("/api/crypto/settings", get(crypto_public_settings))
        .route("/api/crypto/deposit-address", get(crypto_deposit_address))
        .route("/api/crypto/quote/deposit", get(crypto_quote_deposit))
        .route("/api/crypto/quote/withdraw", get(crypto_quote_withdraw))
        .route("/api/crypto/withdraw", post(crypto_withdraw))
        .route("/api/crypto/deposits", get(crypto_list_deposits))
        .route("/api/crypto/withdrawals", get(crypto_list_withdrawals))
        .with_state(state)
}

/// Clear HTTPS — verify seals without the encrypted customer channel.
pub fn dsm_public_router(state: AppState) -> Router {
    Router::new()
        .route("/api/documents/dsm", get(dsm_info))
        .route("/api/documents/dsm/verify", post(dsm_verify))
        .with_state(state)
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn err(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (status, Json(ErrorBody { error: msg.into() })).into_response()
}

fn pdf_response(bytes: Vec<u8>, filename: &str) -> axum::response::Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/pdf"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("inline; filename=\"{filename}\"")).unwrap_or_else(|_| {
            HeaderValue::from_static("inline; filename=\"document.pdf\"")
        }),
    );
    (headers, bytes).into_response()
}

async fn require_session(
    state: &AppState,
    jar: &CookieJar,
    headers: &axum::http::HeaderMap,
) -> Result<passkey_rs::session::Session, axum::response::Response> {
    crate::session_guard::require_bound_session(state, jar, headers)
        .await
        .map(|(s, _)| s)
}

fn statement_needs_step_up(from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> bool {
    let end = to.unwrap_or_else(Utc::now);
    match from {
        None => true,
        Some(start) => (end - start).num_days() > 31,
    }
}

async fn require_party(
    state: &AppState,
    user_id: Uuid,
) -> Result<PartyInfo, axum::response::Response> {
    match auth_profile::get_by_user(&state.auth_db, user_id).await {
        Ok(Some(p)) => Ok(PartyInfo {
            handle: p.handle,
            display_name: p.display_name,
        }),
        Ok(None) => Err(err(
            StatusCode::BAD_REQUEST,
            "Сначала выберите @ник в профиле",
        )),
        Err(e) => Err(err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

fn public_account(a: &ledger::AccountView) -> serde_json::Value {
    let kind = if a.kind == "user_checking" {
        "checking"
    } else {
        a.kind.as_str()
    };
    serde_json::json!({
        "id": a.id,
        "kind": kind,
        "currency": a.currency,
        "status": a.status,
        "balance_minor": a.balance_minor,
    })
}

async fn bank_me(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };

    let profile = match auth_profile::get_by_user(&state.auth_db, session.user_id).await {
        Ok(p) => p,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    // Ensure checking exists once profile is claimed (or auto-provision for username handle).
    let accounts = if profile.is_some() {
        match ledger::ensure_checking_with_welcome(
            &state.ledger_db,
            session.user_id,
            state.welcome_credit_minor,
        )
        .await
        {
            Ok(_) => ledger::list_accounts_for_user(&state.ledger_db, session.user_id)
                .await
                .unwrap_or_default(),
            Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    } else {
        Vec::new()
    };

    Json(serde_json::json!({
        "user_id": session.user_id,
        "username": session.username,
        "profile": profile,
        "accounts": accounts.iter().map(public_account).collect::<Vec<_>>(),
        "currency": "RUB",
    }))
    .into_response()
}

#[derive(Deserialize)]
struct ClaimProfileBody {
    handle: String,
    #[serde(default)]
    display_name: Option<String>,
}

async fn claim_profile(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<ClaimProfileBody>,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = crate::session_guard::require_step_up(&session) {
        return r;
    }

    let handle = match auth_profile::normalize_handle(&body.handle) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let display = body
        .display_name
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| session.username.clone());

    let profile = match auth_profile::upsert_profile(
        &state.auth_db,
        session.user_id,
        &handle,
        &display,
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("bank_profiles_handle_uidx") || msg.contains("duplicate key") {
                return err(StatusCode::CONFLICT, "handle already taken");
            }
            return err(StatusCode::INTERNAL_SERVER_ERROR, msg);
        }
    };

    match ledger::ensure_checking_with_welcome(
        &state.ledger_db,
        session.user_id,
        state.welcome_credit_minor,
    )
    .await
    {
        Ok(acct) => Json(serde_json::json!({
            "profile": profile,
            "account": acct,
        }))
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn list_accounts(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(e) = ledger::ensure_checking_with_welcome(
        &state.ledger_db,
        session.user_id,
        state.welcome_credit_minor,
    )
    .await
    {
        // Only if profile exists — otherwise soft empty.
        if auth_profile::get_by_user(&state.auth_db, session.user_id)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        }
    }
    match ledger::list_accounts_for_user(&state.ledger_db, session.user_id).await {
        Ok(list) => Json(list.iter().map(public_account).collect::<Vec<_>>()).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct TxnQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

async fn account_transactions(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<TxnQuery>,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    match ledger::lookup_account(&state.ledger_db, id, session.user_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return err(StatusCode::NOT_FOUND, "Счёт не найден"),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
    let limit = q.limit.clamp(1, 200);
    match ledger::list_transactions(&state.ledger_db, id, limit).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn transaction_detail(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(entry_id): Path<Uuid>,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let detail = match ledger::get_entry_for_user(&state.ledger_db, session.user_id, entry_id).await
    {
        Ok(Some(d)) => d,
        Ok(None) => return err(StatusCode::NOT_FOUND, "Операция не найдена"),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let counterparty = if let Some(uid) = detail.counterparty_user_id {
        match auth_profile::get_by_user(&state.auth_db, uid).await {
            Ok(Some(p)) => Some(serde_json::json!({
                "handle": p.handle,
                "display_name": p.display_name,
            })),
            Ok(None) => None,
            Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    } else {
        None
    };

    Json(serde_json::json!({
        "entry_id": detail.entry_id,
        "created_at": detail.created_at,
        "memo": detail.memo,
        "status": detail.status,
        "status_label": match detail.status.as_str() {
            "posted" => "Проведена",
            "void" => "Отменена",
            other => other,
        },
        "idempotency_key": detail.idempotency_key,
        "amount_minor": detail.amount_minor,
        "currency": detail.currency,
        "account_id": detail.account_id,
        "direction": detail.direction,
        "direction_label": if detail.amount_minor >= 0 { "Зачисление" } else { "Списание" },
        "counterparty_account_id": detail.counterparty_account_id,
        "counterparty": counterparty,
    }))
    .into_response()
}

#[derive(Deserialize)]
struct StatementQuery {
    account_id: Option<Uuid>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
}

async fn document_statement(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(q): Query<StatementQuery>,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    if statement_needs_step_up(q.from, q.to) {
        if let Err(r) = crate::session_guard::require_step_up(&session) {
            return r;
        }
    }
    let party = match require_party(&state, session.user_id).await {
        Ok(p) => p,
        Err(r) => return r,
    };

    let accounts = match ledger::list_accounts_for_user(&state.ledger_db, session.user_id).await {
        Ok(a) => a,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let account = if let Some(id) = q.account_id {
        match accounts.into_iter().find(|a| a.id == id) {
            Some(a) => a,
            None => return err(StatusCode::NOT_FOUND, "Счёт не найден"),
        }
    } else {
        match accounts
            .into_iter()
            .find(|a| a.kind == "user_checking" && a.currency == "RUB")
        {
            Some(a) => a,
            None => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "Нет открытого рублёвого счёта для выписки",
                );
            }
        }
    };

    let rows = match ledger::list_transactions_range(
        &state.ledger_db,
        account.id,
        200,
        q.from,
        q.to,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    // chronological for statement (oldest first)
    let mut chrono_rows = rows;
    chrono_rows.reverse();

    let Some(dsm) = state.dsm.as_ref() else {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Модуль защиты документов недоступен",
        );
    };
    let now = Utc::now();
    let doc_no = format!(
        "ВЫП-{}-{}",
        now.format("%Y%m%d"),
        &account.id.to_string()[..8]
    );
    let doc_ref = format!(
        "{}:{}:{}",
        account.id,
        q.from.map(|d| d.timestamp()).unwrap_or(0),
        q.to.map(|d| d.timestamp()).unwrap_or(0),
    );
    let process_data =
        documents::statement_process_data(&account.id.to_string(), chrono_rows.len(), &doc_no);
    let seal = match dsm
        .seal_or_get("statement", &doc_ref, "Kontoauszug-V1", &process_data)
        .await
    {
        Ok(s) => s,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };

    match documents::render_statement(&party, &account, &chrono_rows, q.from, q.to, &seal) {
        Ok(pdf) => {
            let name = format!("laefye-statement-{}.pdf", now.format("%Y%m%d"));
            pdf_response(pdf, &name)
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn document_receipt(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Path(entry_id): Path<Uuid>,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let party = match require_party(&state, session.user_id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let detail = match ledger::get_entry_for_user(&state.ledger_db, session.user_id, entry_id).await
    {
        Ok(Some(d)) => d,
        Ok(None) => return err(StatusCode::NOT_FOUND, "Операция не найдена"),
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    let counterparty = if let Some(uid) = detail.counterparty_user_id {
        match auth_profile::get_by_user(&state.auth_db, uid).await {
            Ok(Some(p)) => Some(PartyInfo {
                handle: p.handle,
                display_name: p.display_name,
            }),
            Ok(None) => None,
            Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    } else {
        None
    };

    let Some(dsm) = state.dsm.as_ref() else {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Модуль защиты документов недоступен",
        );
    };
    let process_data = documents::receipt_process_data(
        detail.amount_minor,
        &detail.currency,
        &detail.entry_id.to_string(),
        &party.handle,
    );
    let seal = match dsm
        .seal_or_get(
            "receipt",
            &detail.entry_id.to_string(),
            "Kassenbeleg-V1",
            &process_data,
        )
        .await
    {
        Ok(s) => s,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e),
    };

    match documents::render_receipt(&party, &detail, counterparty.as_ref(), &seal) {
        Ok(pdf) => {
            let name = format!("laefye-receipt-{}.pdf", &entry_id.to_string()[..8]);
            pdf_response(pdf, &name)
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Serialize)]
struct DsmInfo {
    serial: String,
    public_key_b64: String,
    sig_alg: &'static str,
    note: &'static str,
}

async fn dsm_info(State(state): State<AppState>) -> impl IntoResponse {
    let Some(dsm) = state.dsm.as_ref() else {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Модуль защиты документов недоступен",
        );
    };
    Json(DsmInfo {
        serial: dsm.serial().into(),
        public_key_b64: dsm.public_key_b64().into(),
        sig_alg: dsm.sig_alg(),
        note: "Laefye DSM — программный SE по мотивам DE TSE / DSFinV-K (не сертифицированный BSI TSE)",
    })
    .into_response()
}

#[derive(Deserialize)]
struct VerifyBody {
    qr_payload: String,
}

async fn dsm_verify(
    State(state): State<AppState>,
    Json(body): Json<VerifyBody>,
) -> impl IntoResponse {
    let Some(dsm) = state.dsm.as_ref() else {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Модуль защиты документов недоступен",
        );
    };
    match dsm.verify_qr_payload_async(body.qr_payload.trim()).await {
        Ok(r) => Json(r).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
struct TransferBody {
    to_handle: String,
    amount_minor: i64,
    #[serde(default = "rub")]
    currency: String,
    idempotency_key: String,
    #[serde(default)]
    memo: String,
}

fn rub() -> String {
    "RUB".into()
}

async fn create_transfer(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<TransferBody>,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = crate::session_guard::require_step_up(&session) {
        return r;
    }

    let handle = match auth_profile::normalize_handle(&body.to_handle) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };

    let to = match auth_profile::get_by_handle(&state.auth_db, &handle).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return err(
                StatusCode::NOT_FOUND,
                format!("Пользователь @{handle} не найден"),
            );
        }
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };

    // Ensure both have checking accounts.
    if let Err(e) = ledger::ensure_checking_with_welcome(
        &state.ledger_db,
        session.user_id,
        state.welcome_credit_minor,
    )
    .await
    {
        return err(StatusCode::BAD_REQUEST, format!("sender account: {e}"));
    }
    if let Err(e) = ledger::ensure_checking_with_welcome(
        &state.ledger_db,
        to.user_id,
        state.welcome_credit_minor,
    )
    .await
    {
        return err(StatusCode::BAD_REQUEST, format!("recipient account: {e}"));
    }

    let memo = if body.memo.trim().is_empty() {
        format!("Transfer to @{handle}")
    } else {
        body.memo.trim().chars().take(200).collect()
    };

    let amount_minor = body.amount_minor;
    let currency = body.currency.clone();
    let req = TransferRequest {
        from_user_id: session.user_id,
        to_user_id: to.user_id,
        amount_minor,
        currency: currency.clone(),
        idempotency_key: body.idempotency_key,
        memo,
        created_by: session.user_id,
    };

    match ledger::transfer_p2p(&state.ledger_db, req).await {
        Ok(res) => {
            if !res.idempotent_replay {
                let amount = format!("{:.2} {currency}", amount_minor as f64 / 100.0);
                let _ = crate::notifications::push(
                    &state.auth_db,
                    to.user_id,
                    "transfer",
                    "Входящий перевод",
                    &format!("Вам перевели {amount}"),
                    Some("/app/history"),
                )
                .await;
            }
            Json(res).into_response()
        }
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("insufficient")
                || msg.contains("yourself")
                || msg.contains("amount")
                || msg.contains("idempotency")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            err(status, msg)
        }
    }
}

#[derive(Deserialize)]
struct LookupQuery {
    handle: String,
}

async fn lookup_handle(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(q): Query<LookupQuery>,
) -> impl IntoResponse {
    let _ = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let handle = match auth_profile::normalize_handle(&q.handle) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    match auth_profile::get_by_handle(&state.auth_db, &handle).await {
        Ok(Some(p)) => Json(serde_json::json!({
            "handle": p.handle,
            "display_name": p.display_name,
        }))
        .into_response(),
        Ok(None) => err(
            StatusCode::NOT_FOUND,
            format!("Пользователь @{handle} не найден"),
        ),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct OpenAccountBody {
    currency: String,
}

async fn open_account(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<OpenAccountBody>,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let ccy = body.currency.to_ascii_uppercase();
    let settings = match crate::fx::load_settings(&state.ledger_db).await {
        Ok(s) => s,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if !settings.enabled_currencies.iter().any(|c| c == &ccy) {
        return err(StatusCode::BAD_REQUEST, "currency not enabled");
    }
    match ledger::ensure_checking(&state.ledger_db, session.user_id, &ccy, 0).await {
        Ok(a) => Json(a).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

#[derive(Deserialize)]
struct QuoteQuery {
    base: String,
    quote: String,
}

async fn fx_quote(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(q): Query<QuoteQuery>,
) -> impl IntoResponse {
    if let Err(r) = require_session(&state, &jar, &headers).await {
        return r;
    }
    match crate::fx::quote_pair(&state.ledger_db, &q.base, &q.quote).await {
        Ok(q) => Json(serde_json::json!({
            "base": q.base,
            "quote": q.quote,
            "mid": q.mid,
            "buy": q.buy,
            "sell": q.sell,
            "as_of": q.as_of,
        }))
        .into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

#[derive(Deserialize)]
struct ConvertBody {
    from_currency: String,
    to_currency: String,
    from_amount_minor: i64,
    idempotency_key: String,
}

async fn fx_convert(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<ConvertBody>,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = crate::session_guard::require_step_up(&session) {
        return r;
    }
    if let Err(e) = ledger::ensure_checking(
        &state.ledger_db,
        session.user_id,
        &body.from_currency,
        0,
    )
    .await
    {
        return err(StatusCode::BAD_REQUEST, e.to_string());
    }
    if let Err(e) =
        ledger::ensure_checking(&state.ledger_db, session.user_id, &body.to_currency, 0).await
    {
        return err(StatusCode::BAD_REQUEST, e.to_string());
    }
    match ledger::convert_currency(
        &state.ledger_db,
        session.user_id,
        &body.from_currency,
        &body.to_currency,
        body.from_amount_minor,
        &body.idempotency_key,
    )
    .await
    {
        Ok(r) => Json(r).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn kyc_status(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    match crate::kyc::get_or_create_draft(&state.auth_db, session.user_id).await {
        Ok(c) => {
            let docs: Vec<(Uuid, String, i32)> = sqlx::query_as(
                "SELECT id, doc_type, byte_size FROM kyc_documents WHERE case_id = $1",
            )
            .bind(c.id)
            .fetch_all(&state.auth_db)
            .await
            .unwrap_or_default();
            Json(serde_json::json!({
                "case": {
                    "status": c.status,
                    "note": c.reviewer_note,
                },
                "documents": docs.into_iter().map(|(id, doc_type, byte_size)| {
                    serde_json::json!({ "id": id, "doc_type": doc_type, "byte_size": byte_size })
                }).collect::<Vec<_>>(),
                "uploads_enabled": state.kyc.is_some(),
            }))
            .into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn kyc_submit(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let case = match crate::kyc::get_or_create_draft(&state.auth_db, session.user_id).await {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    match crate::kyc::submit_case(&state.auth_db, session.user_id, case.id).await {
        Ok(c) => Json(c).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn kyc_upload(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    mut multipart: axum::extract::Multipart,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Some(store) = &state.kyc else {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Загрузка документов временно недоступна",
        );
    };
    let case = match crate::kyc::get_or_create_draft(&state.auth_db, session.user_id).await {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if !matches!(case.status.as_str(), "draft" | "resubmit") {
        return err(
            StatusCode::BAD_REQUEST,
            "В текущем статусе нельзя загружать документы",
        );
    }

    let mut doc_type = String::from("other");
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut content_type = String::from("application/octet-stream");

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "doc_type" {
            if let Ok(t) = field.text().await {
                doc_type = t;
            }
        } else if name == "file" {
            content_type = field
                .content_type()
                .map(|m| m.to_string())
                .unwrap_or_else(|| "application/octet-stream".into());
            match field.bytes().await {
                Ok(b) => file_bytes = Some(b.to_vec()),
                Err(e) => return err(StatusCode::BAD_REQUEST, e.to_string()),
            }
        }
    }

    let Some(bytes) = file_bytes else {
        return err(StatusCode::BAD_REQUEST, "file required");
    };

    match crate::kyc::add_document(
        &state.auth_db,
        store,
        session.user_id,
        case.id,
        &doc_type,
        &content_type,
        &bytes,
    )
    .await
    {
        Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn market_snapshot(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = require_session(&state, &jar, &headers).await {
        return r;
    }
    match crate::market::load_snapshot(&state.ledger_db).await {
        Ok(s) => Json(serde_json::json!({
            "usdt_rub": s.usdt_rub_mid,
            "usdt_usd": s.usdt_usd_mid,
            "updated_at": s.rates_updated_at,
            "cbr": {
                "usd_rub": s.cbr_usd_rub,
                "key_rate": s.cbr_key_rate,
                "inflation": s.cbr_inflation,
            },
        }))
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn crypto_public_settings(State(state): State<AppState>, jar: CookieJar,
    headers: HeaderMap) -> impl IntoResponse {
    if let Err(r) = require_session(&state, &jar, &headers).await {
        return r;
    }
    match crate::crypto::load_settings(&state.ledger_db).await {
        Ok(s) => Json(serde_json::json!({
            "chain": s.chain,
            "asset": s.asset,
            "network": "TRC-20",
            "usdt_rub": s.usdt_rub_mid,
            "min_deposit_usdt_minor": s.min_deposit_usdt_minor,
            "min_withdraw_usdt_minor": s.min_withdraw_usdt_minor,
            "enabled": s.enabled,
        }))
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn crypto_deposit_address(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    let Some(seed) = state.crypto.master_seed else {
        return err(
            StatusCode::SERVICE_UNAVAILABLE,
            "deposit addresses unavailable (set TRON_MASTER_SEED)",
        );
    };
    // Ensure RUB account exists for incoming credits.
    if let Err(e) = ledger::ensure_checking(&state.ledger_db, session.user_id, "RUB", 0).await {
        return err(StatusCode::BAD_REQUEST, e.to_string());
    }
    match crate::crypto::ensure_deposit_address(&state.ledger_db, session.user_id, &seed).await {
        Ok(a) => Json(serde_json::json!({
            "address": a.address,
            "chain": "tron",
            "asset": "USDT",
            "network": "TRC-20",
        }))
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct UsdtQuoteQuery {
    usdt_minor: Option<i64>,
    rub_minor: Option<i64>,
}

async fn crypto_quote_deposit(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(q): Query<UsdtQuoteQuery>,
) -> impl IntoResponse {
    if let Err(r) = require_session(&state, &jar, &headers).await {
        return r;
    }
    let Some(usdt) = q.usdt_minor else {
        return err(StatusCode::BAD_REQUEST, "usdt_minor required");
    };
    match crate::crypto::quote_deposit(&state.ledger_db, usdt).await {
        Ok(q) => Json(serde_json::json!({
            "usdt_minor": q.usdt_minor,
            "rub_credit_minor": q.rub_credit_minor,
            "fee_minor": q.fee_minor,
            "usdt_rub": q.usdt_rub_mid,
            "min_usdt_minor": q.min_usdt_minor,
        }))
        .into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn crypto_quote_withdraw(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Query(q): Query<UsdtQuoteQuery>,
) -> impl IntoResponse {
    if let Err(r) = require_session(&state, &jar, &headers).await {
        return r;
    }
    let Some(rub) = q.rub_minor else {
        return err(StatusCode::BAD_REQUEST, "rub_minor required");
    };
    match crate::crypto::quote_withdraw(&state.ledger_db, rub).await {
        Ok(q) => Json(serde_json::json!({
            "rub_debit_minor": q.rub_debit_minor,
            "usdt_out_minor": q.usdt_out_minor,
            "fee_minor": q.fee_minor,
            "usdt_rub": q.usdt_rub_mid,
            "min_usdt_minor": q.min_usdt_minor,
        }))
        .into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

#[derive(Deserialize)]
struct WithdrawBody {
    rub_minor: i64,
    to_address: String,
    idempotency_key: String,
}

async fn crypto_withdraw(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Json(body): Json<WithdrawBody>,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    if let Err(r) = crate::session_guard::require_step_up(&session) {
        return r;
    }
    if !crate::crypto::is_valid_tron_address(&body.to_address) {
        return err(StatusCode::BAD_REQUEST, "invalid TRON address");
    }
    match crate::crypto::queue_withdraw(
        &state.ledger_db,
        session.user_id,
        body.rub_minor,
        &body.to_address,
        &body.idempotency_key,
    )
    .await
    {
        Ok(r) => Json(r).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn crypto_list_deposits(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    match crate::crypto::list_deposits(&state.ledger_db, session.user_id, 50).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn crypto_list_withdrawals(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
) -> impl IntoResponse {
    let session = match require_session(&state, &jar, &headers).await {
        Ok(s) => s,
        Err(r) => return r,
    };
    match crate::crypto::list_withdrawals(&state.ledger_db, session.user_id, 50).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
