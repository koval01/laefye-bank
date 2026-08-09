//! Thin Rust façade over PostgreSQL ledger / FX engine.
//! All money math, fees, locking and postings live in SQL (`003` + `008_ledger_paranoid`).

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use super::CURRENCY_RUB;

const SERIALIZABLE_ATTEMPTS: u32 = 10;

fn is_retryable_tx_error(e: &sqlx::Error) -> bool {
    if let Some(db) = e.as_database_error() {
        if let Some(code) = db.code() {
            if matches!(code.as_ref(), "40001" | "40P01") {
                return true;
            }
        }
        let msg = db.message();
        if msg.contains("could not serialize") || msg.contains("deadlock detected") {
            return true;
        }
    }
    let s = e.to_string();
    s.contains("could not serialize") || s.contains("deadlock detected")
}

#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl LedgerError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }

    pub fn from_db(e: sqlx::Error) -> Self {
        // Surface RAISE EXCEPTION text to API clients.
        let msg = e.to_string();
        if let Some(db) = e.as_database_error() {
            if let Some(m) = db.message().split("ERROR:").last() {
                return Self::Message(m.trim().to_string());
            }
            return Self::Message(db.message().to_string());
        }
        // Often: error returned from database: ...
        if let Some(idx) = msg.find("ERROR:") {
            return Self::Message(msg[idx + 6..].trim().to_string());
        }
        Self::Db(e)
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AccountView {
    pub id: Uuid,
    pub owner_user_id: Option<Uuid>,
    pub kind: String,
    pub currency: String,
    pub status: String,
    pub balance_minor: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TxnView {
    pub entry_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub memo: String,
    pub amount_minor: i64,
    pub currency: String,
    pub counterparty_account_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TxnDetail {
    pub entry_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub memo: String,
    pub status: String,
    pub idempotency_key: String,
    pub amount_minor: i64,
    pub currency: String,
    pub account_id: Uuid,
    pub counterparty_account_id: Option<Uuid>,
    pub counterparty_user_id: Option<Uuid>,
    pub direction: String,
}

#[derive(Debug, Clone)]
pub struct TransferRequest {
    pub from_user_id: Uuid,
    pub to_user_id: Uuid,
    pub amount_minor: i64,
    pub currency: String,
    pub idempotency_key: String,
    pub memo: String,
    pub created_by: Uuid,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TransferResult {
    pub entry_id: Uuid,
    pub idempotent_replay: bool,
    pub from_balance_minor: i64,
    pub to_balance_minor: i64,
    pub fee_minor: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FxConvertResult {
    pub from_entry_id: Uuid,
    pub to_entry_id: Uuid,
    pub from_amount_minor: i64,
    pub to_amount_minor: i64,
    pub applied_rate: String,
    pub mid_rate: String,
    pub fee_minor: i64,
}

pub async fn list_accounts_for_user(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<AccountView>, LedgerError> {
    let rows = sqlx::query_as::<_, AccountView>(
        r#"
        SELECT a.id, a.owner_user_id, a.kind, a.currency, a.status,
               COALESCE(account_balance(a.id), 0) AS balance_minor
          FROM accounts a
         WHERE a.owner_user_id = $1
         ORDER BY a.created_at
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
    .map_err(LedgerError::from_db)?;
    Ok(rows)
}

pub async fn lookup_account(
    pool: &PgPool,
    account_id: Uuid,
    owner_user_id: Uuid,
) -> Result<Option<AccountView>, LedgerError> {
    let row = sqlx::query_as::<_, AccountView>(
        r#"
        SELECT a.id, a.owner_user_id, a.kind, a.currency, a.status,
               COALESCE(account_balance(a.id), 0) AS balance_minor
          FROM accounts a
         WHERE a.id = $1 AND a.owner_user_id = $2
        "#,
    )
    .bind(account_id)
    .bind(owner_user_id)
    .fetch_optional(pool)
    .await
    .map_err(LedgerError::from_db)?;
    Ok(row)
}

pub async fn list_transactions(
    pool: &PgPool,
    account_id: Uuid,
    limit: i64,
) -> Result<Vec<TxnView>, LedgerError> {
    list_transactions_range(pool, account_id, limit, None, None).await
}

pub async fn list_transactions_range(
    pool: &PgPool,
    account_id: Uuid,
    limit: i64,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<Vec<TxnView>, LedgerError> {
    let rows = sqlx::query(
        r#"
        SELECT je.id AS entry_id,
               je.created_at,
               je.memo,
               SUM(p.amount_minor)::bigint AS amount_minor,
               MAX(p.currency) AS currency,
               (
                 SELECT p2.account_id
                   FROM postings p2
                  WHERE p2.entry_id = je.id AND p2.account_id <> $1
                  ORDER BY p2.id
                  LIMIT 1
               ) AS counterparty_account_id
          FROM postings p
          JOIN journal_entries je ON je.id = p.entry_id
         WHERE p.account_id = $1
           AND je.status = 'posted'
           AND ($2::timestamptz IS NULL OR je.created_at >= $2)
           AND ($3::timestamptz IS NULL OR je.created_at < $3)
         GROUP BY je.id, je.created_at, je.memo
         ORDER BY je.created_at DESC
         LIMIT $4
        "#,
    )
    .bind(account_id)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(LedgerError::from_db)?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        use sqlx::Row;
        out.push(TxnView {
            entry_id: r.get("entry_id"),
            created_at: r.get("created_at"),
            memo: r.get("memo"),
            amount_minor: r.get("amount_minor"),
            currency: r.get("currency"),
            counterparty_account_id: r.get("counterparty_account_id"),
        });
    }
    Ok(out)
}

/// Load a journal entry as seen from one of the caller's accounts.
pub async fn get_entry_for_user(
    pool: &PgPool,
    user_id: Uuid,
    entry_id: Uuid,
) -> Result<Option<TxnDetail>, LedgerError> {
    let row = sqlx::query(
        r#"
        SELECT je.id AS entry_id,
               je.created_at,
               je.memo,
               je.status,
               je.idempotency_key,
               p.amount_minor,
               p.currency,
               p.account_id,
               (
                 SELECT p2.account_id
                   FROM postings p2
                  WHERE p2.entry_id = je.id AND p2.account_id <> p.account_id
                  ORDER BY p2.id
                  LIMIT 1
               ) AS counterparty_account_id,
               (
                 SELECT a2.owner_user_id
                   FROM postings p2
                   JOIN accounts a2 ON a2.id = p2.account_id
                  WHERE p2.entry_id = je.id AND p2.account_id <> p.account_id
                  ORDER BY p2.id
                  LIMIT 1
               ) AS counterparty_user_id
          FROM journal_entries je
          JOIN postings p ON p.entry_id = je.id
          JOIN accounts a ON a.id = p.account_id
         WHERE je.id = $1
           AND a.owner_user_id = $2
           AND a.kind = 'user_checking'
         ORDER BY p.id
         LIMIT 1
        "#,
    )
    .bind(entry_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(LedgerError::from_db)?;

    let Some(r) = row else {
        return Ok(None);
    };
    use sqlx::Row;
    let amount_minor: i64 = r.get("amount_minor");
    Ok(Some(TxnDetail {
        entry_id: r.get("entry_id"),
        created_at: r.get("created_at"),
        memo: r.get("memo"),
        status: r.get("status"),
        idempotency_key: r.get("idempotency_key"),
        amount_minor,
        currency: r.get("currency"),
        account_id: r.get("account_id"),
        counterparty_account_id: r.get("counterparty_account_id"),
        counterparty_user_id: r.get("counterparty_user_id"),
        direction: if amount_minor >= 0 {
            "credit".into()
        } else {
            "debit".into()
        },
    }))
}

pub async fn ensure_checking_with_welcome(
    pool: &PgPool,
    user_id: Uuid,
    welcome_minor: i64,
) -> Result<AccountView, LedgerError> {
    ensure_checking(pool, user_id, CURRENCY_RUB, welcome_minor).await
}

pub async fn ensure_checking(
    pool: &PgPool,
    user_id: Uuid,
    currency: &str,
    welcome_minor: i64,
) -> Result<AccountView, LedgerError> {
    let id: Uuid = sqlx::query_scalar("SELECT ledger_ensure_checking($1, $2, $3)")
        .bind(user_id)
        .bind(currency.to_ascii_uppercase())
        .bind(welcome_minor)
        .fetch_one(pool)
        .await
        .map_err(LedgerError::from_db)?;

    lookup_account(pool, id, user_id)
        .await?
        .ok_or_else(|| LedgerError::msg("account missing after provision"))
}

pub async fn transfer_p2p(
    pool: &PgPool,
    req: TransferRequest,
) -> Result<TransferResult, LedgerError> {
    let mut last = LedgerError::msg("serializable retry exhausted");
    for attempt in 0..SERIALIZABLE_ATTEMPTS {
        let mut tx = pool.begin().await.map_err(LedgerError::from_db)?;
        if let Err(e) = sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await
        {
            let _ = tx.rollback().await;
            return Err(LedgerError::from_db(e));
        }

        let result = sqlx::query_as::<_, TransferResult>(
            r#"
            SELECT entry_id, idempotent_replay, from_balance_minor, to_balance_minor, fee_minor
              FROM ledger_transfer_p2p($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(req.from_user_id)
        .bind(req.to_user_id)
        .bind(req.amount_minor)
        .bind(req.currency.to_ascii_uppercase())
        .bind(&req.idempotency_key)
        .bind(&req.memo)
        .bind(req.created_by)
        .fetch_one(&mut *tx)
        .await;

        match result {
            Ok(row) => {
                tx.commit().await.map_err(LedgerError::from_db)?;
                return Ok(row);
            }
            Err(e) if is_retryable_tx_error(&e) && attempt + 1 < SERIALIZABLE_ATTEMPTS => {
                let _ = tx.rollback().await;
                last = LedgerError::from_db(e);
                let ms = 5u64.saturating_mul(1u64 << attempt.min(6));
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
            Err(e) => {
                let _ = tx.rollback().await;
                return Err(LedgerError::from_db(e));
            }
        }
    }
    Err(last)
}

pub async fn convert_currency(
    pool: &PgPool,
    user_id: Uuid,
    from_ccy: &str,
    to_ccy: &str,
    from_amount_minor: i64,
    idempotency_key: &str,
) -> Result<FxConvertResult, LedgerError> {
    let from_ccy = from_ccy.to_ascii_uppercase();
    let to_ccy = to_ccy.to_ascii_uppercase();
    let mut last = LedgerError::msg("serializable retry exhausted");
    for attempt in 0..SERIALIZABLE_ATTEMPTS {
        let mut tx = pool.begin().await.map_err(LedgerError::from_db)?;
        if let Err(e) = sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await
        {
            let _ = tx.rollback().await;
            return Err(LedgerError::from_db(e));
        }

        let result = sqlx::query_as::<_, FxConvertResult>(
            r#"
            SELECT from_entry_id, to_entry_id, from_amount_minor, to_amount_minor,
                   applied_rate, mid_rate, fee_minor
              FROM fx_convert($1, $2, $3, $4, $5)
            "#,
        )
        .bind(user_id)
        .bind(&from_ccy)
        .bind(&to_ccy)
        .bind(from_amount_minor)
        .bind(idempotency_key)
        .fetch_one(&mut *tx)
        .await;

        match result {
            Ok(row) => {
                tx.commit().await.map_err(LedgerError::from_db)?;
                return Ok(row);
            }
            Err(e) if is_retryable_tx_error(&e) && attempt + 1 < SERIALIZABLE_ATTEMPTS => {
                let _ = tx.rollback().await;
                last = LedgerError::from_db(e);
                let ms = 5u64.saturating_mul(1u64 << attempt.min(6));
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
            Err(e) => {
                let _ = tx.rollback().await;
                return Err(LedgerError::from_db(e));
            }
        }
    }
    Err(last)
}
