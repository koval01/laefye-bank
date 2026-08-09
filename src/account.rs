//! Customer account center: trust, limits, auth events, data requests.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct TrustInfo {
    pub level: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LimitsInfo {
    pub daily_transfer_minor: i64,
    pub single_transfer_minor: i64,
    pub daily_withdraw_minor: i64,
    pub crypto_enabled: bool,
    pub currency: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AuthEvent {
    pub id: Uuid,
    pub event_type: String,
    pub ip_hint: String,
    pub user_agent: String,
    pub detail: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DataRequest {
    pub id: Uuid,
    pub request_type: String,
    pub payload: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub fn trust_from_kyc(kyc_status: &str) -> TrustInfo {
    match kyc_status {
        "approved" => TrustInfo {
            level: "verified".into(),
            label: "Проверенный клиент".into(),
            description: "Личность подтверждена. Доступны повышенные лимиты.".into(),
        },
        "pending" => TrustInfo {
            level: "review".into(),
            label: "На проверке".into(),
            description: "Документы на рассмотрении. Пока действуют базовые лимиты.".into(),
        },
        "resubmit" => TrustInfo {
            level: "attention".into(),
            label: "Требуется доработка".into(),
            description: "Нужно обновить документы для подтверждения личности.".into(),
        },
        "rejected" => TrustInfo {
            level: "restricted".into(),
            label: "Ограничен".into(),
            description: "Верификация отклонена. Обратитесь в поддержку или подайте документы снова.".into(),
        },
        _ => TrustInfo {
            level: "basic".into(),
            label: "Базовый".into(),
            description: "Аккаунт открыт. Пройдите верификацию, чтобы снять лимиты.".into(),
        },
    }
}

pub fn limits_from_kyc(kyc_status: &str) -> LimitsInfo {
    match kyc_status {
        "approved" => LimitsInfo {
            daily_transfer_minor: 500_000_00, // ₽500 000
            single_transfer_minor: 200_000_00,
            daily_withdraw_minor: 300_000_00,
            crypto_enabled: true,
            currency: "RUB".into(),
            note: "Лимиты для верифицированного клиента.".into(),
        },
        "pending" | "resubmit" => LimitsInfo {
            daily_transfer_minor: 100_000_00,
            single_transfer_minor: 50_000_00,
            daily_withdraw_minor: 30_000_00,
            crypto_enabled: true,
            currency: "RUB".into(),
            note: "Временные лимиты на период проверки документов.".into(),
        },
        "rejected" => LimitsInfo {
            daily_transfer_minor: 10_000_00,
            single_transfer_minor: 5_000_00,
            daily_withdraw_minor: 0,
            crypto_enabled: false,
            currency: "RUB".into(),
            note: "Вывод и крупные операции ограничены.".into(),
        },
        _ => LimitsInfo {
            daily_transfer_minor: 50_000_00,
            single_transfer_minor: 25_000_00,
            daily_withdraw_minor: 15_000_00,
            crypto_enabled: true,
            currency: "RUB".into(),
            note: "Базовые лимиты до прохождения верификации.".into(),
        },
    }
}

pub async fn record_event(
    pool: &PgPool,
    user_id: Uuid,
    event_type: &str,
    ip_hint: &str,
    user_agent: &str,
    detail: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO auth_events (user_id, event_type, ip_hint, user_agent, detail)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(user_id)
    .bind(event_type)
    .bind(ip_hint)
    .bind(user_agent)
    .bind(detail)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_events(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
) -> Result<Vec<AuthEvent>, sqlx::Error> {
    sqlx::query_as::<_, AuthEvent>(
        r#"
        SELECT id, event_type, ip_hint, user_agent, detail, created_at
          FROM auth_events
         WHERE user_id = $1
         ORDER BY created_at DESC
         LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
}

pub async fn create_data_request(
    pool: &PgPool,
    user_id: Uuid,
    request_type: &str,
    payload: &str,
) -> Result<DataRequest, sqlx::Error> {
    sqlx::query_as::<_, DataRequest>(
        r#"
        INSERT INTO profile_data_requests (user_id, request_type, payload)
        VALUES ($1, $2, $3)
        RETURNING id, request_type, payload, status, created_at, updated_at
        "#,
    )
    .bind(user_id)
    .bind(request_type)
    .bind(payload)
    .fetch_one(pool)
    .await
}

pub async fn list_data_requests(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<DataRequest>, sqlx::Error> {
    sqlx::query_as::<_, DataRequest>(
        r#"
        SELECT id, request_type, payload, status, created_at, updated_at
          FROM profile_data_requests
         WHERE user_id = $1
         ORDER BY created_at DESC
         LIMIT 50
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

pub async fn user_registered_at(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar::<_, DateTime<Utc>>(
        "SELECT created_at FROM passkey_users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}
