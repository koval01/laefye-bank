//! USDT TRC-20 (TRON) on/off-ramp — thin shell over SQL + chain watcher.

mod address;
mod trongrid;
mod worker;

pub use address::{derive_deposit_address, hot_wallet_address, is_valid_tron_address, TronKey};
pub use worker::spawn_crypto_workers;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl CryptoError {
    pub fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }

    fn from_db(e: sqlx::Error) -> Self {
        let msg = e.to_string();
        if let Some(db) = e.as_database_error() {
            return Self::Message(db.message().to_string());
        }
        if let Some(idx) = msg.find("ERROR:") {
            return Self::Message(msg[idx + 6..].trim().to_string());
        }
        Self::Db(e)
    }
}

#[derive(Debug, Clone)]
pub struct CryptoConfig {
    pub mode: CryptoMode,
    pub trongrid_base: String,
    pub trongrid_api_key: Option<String>,
    /// 32-byte hex seed for HD-ish deposit derivation + hot wallet (index 0).
    pub master_seed: Option<[u8; 32]>,
    pub poll_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoMode {
    /// No chain IO — deposit simulation via admin API only; withdraws auto-mock.
    Mock,
    /// Watch TronGrid + broadcast real TRC-20 transfers when seed is set.
    Mainnet,
}

impl CryptoMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "mainnet" | "tron" | "live" => Self::Mainnet,
            _ => Self::Mock,
        }
    }
}

impl Default for CryptoConfig {
    fn default() -> Self {
        Self {
            mode: CryptoMode::Mock,
            trongrid_base: "https://api.trongrid.io".into(),
            trongrid_api_key: None,
            master_seed: None,
            poll_secs: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CryptoSettings {
    pub chain: String,
    pub asset: String,
    pub usdt_contract: String,
    pub usdt_usd_mid: rust_decimal::Decimal,
    pub usdt_rub_mid: rust_decimal::Decimal,
    pub usdt_rub_source: String,
    pub rates_updated_at: Option<DateTime<Utc>>,
    pub deposit_spread_bps: i32,
    pub withdraw_spread_bps: i32,
    pub min_deposit_usdt_minor: i64,
    pub min_withdraw_usdt_minor: i64,
    pub confirmations_required: i32,
    pub hot_wallet_address: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DepositAddress {
    pub id: Uuid,
    pub user_id: Uuid,
    pub address: String,
    pub derivation_index: i32,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DepositQuote {
    pub usdt_minor: i64,
    pub rub_gross_minor: i64,
    pub rub_credit_minor: i64,
    pub fee_minor: i64,
    pub usdt_usd_mid: rust_decimal::Decimal,
    pub usd_rub_mid: rust_decimal::Decimal,
    pub usdt_rub_mid: rust_decimal::Decimal,
    pub spread_bps: i32,
    pub min_usdt_minor: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WithdrawQuote {
    pub rub_debit_minor: i64,
    pub usdt_out_minor: i64,
    pub fee_minor: i64,
    pub usdt_usd_mid: rust_decimal::Decimal,
    pub usd_rub_mid: rust_decimal::Decimal,
    pub usdt_rub_mid: rust_decimal::Decimal,
    pub spread_bps: i32,
    pub min_usdt_minor: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WithdrawResult {
    pub withdrawal_id: Uuid,
    pub journal_entry_id: Option<Uuid>,
    pub usdt_out_minor: i64,
    pub fee_minor: i64,
    pub rub_debit_minor: i64,
    pub status: String,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct DepositRow {
    pub id: Uuid,
    pub tx_hash: String,
    pub usdt_minor: i64,
    pub rub_credit_minor: i64,
    pub fee_minor: i64,
    pub status: String,
    pub detected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WithdrawRow {
    pub id: Uuid,
    pub to_address: String,
    pub rub_debit_minor: i64,
    pub usdt_out_minor: i64,
    pub fee_minor: i64,
    pub status: String,
    pub tx_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn load_settings(pool: &PgPool) -> Result<CryptoSettings, CryptoError> {
    sqlx::query_as::<_, CryptoSettings>(
        r#"
        SELECT chain, asset, usdt_contract, usdt_usd_mid,
               COALESCE(usdt_rub_mid, 0) AS usdt_rub_mid,
               COALESCE(usdt_rub_source, 'unset') AS usdt_rub_source,
               rates_updated_at,
               deposit_spread_bps, withdraw_spread_bps,
               min_deposit_usdt_minor, min_withdraw_usdt_minor,
               confirmations_required, hot_wallet_address, enabled
          FROM crypto_settings WHERE id = 1
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(CryptoError::from_db)
}

pub async fn update_settings(
    pool: &PgPool,
    usdt_usd_mid: rust_decimal::Decimal,
    deposit_spread_bps: i32,
    withdraw_spread_bps: i32,
    min_deposit_usdt_minor: i64,
    min_withdraw_usdt_minor: i64,
    enabled: bool,
    updated_by: Option<Uuid>,
) -> Result<CryptoSettings, CryptoError> {
    sqlx::query(
        r#"
        UPDATE crypto_settings SET
            usdt_usd_mid = $1,
            deposit_spread_bps = $2,
            withdraw_spread_bps = $3,
            min_deposit_usdt_minor = $4,
            min_withdraw_usdt_minor = $5,
            enabled = $6,
            updated_at = NOW(),
            updated_by = $7
         WHERE id = 1
        "#,
    )
    .bind(usdt_usd_mid)
    .bind(deposit_spread_bps)
    .bind(withdraw_spread_bps)
    .bind(min_deposit_usdt_minor)
    .bind(min_withdraw_usdt_minor)
    .bind(enabled)
    .bind(updated_by)
    .execute(pool)
    .await
    .map_err(CryptoError::from_db)?;

    // Keep fee_rules bps in sync so calc_fee floor matches admin UI.
    sqlx::query("UPDATE fee_rules SET bps = $1 WHERE kind = 'crypto_deposit'")
        .bind(deposit_spread_bps)
        .execute(pool)
        .await
        .map_err(CryptoError::from_db)?;
    sqlx::query("UPDATE fee_rules SET bps = $1 WHERE kind = 'crypto_withdraw'")
        .bind(withdraw_spread_bps)
        .execute(pool)
        .await
        .map_err(CryptoError::from_db)?;

    load_settings(pool).await
}

pub async fn set_hot_wallet_address(pool: &PgPool, address: &str) -> Result<(), CryptoError> {
    sqlx::query("UPDATE crypto_settings SET hot_wallet_address = $1, updated_at = NOW() WHERE id = 1")
        .bind(address)
        .execute(pool)
        .await
        .map_err(CryptoError::from_db)?;
    Ok(())
}

pub async fn quote_deposit(pool: &PgPool, usdt_minor: i64) -> Result<DepositQuote, CryptoError> {
    sqlx::query_as::<_, DepositQuote>(
        "SELECT * FROM crypto_quote_deposit($1)",
    )
    .bind(usdt_minor)
    .fetch_one(pool)
    .await
    .map_err(CryptoError::from_db)
}

pub async fn quote_withdraw(pool: &PgPool, rub_minor: i64) -> Result<WithdrawQuote, CryptoError> {
    sqlx::query_as::<_, WithdrawQuote>(
        "SELECT * FROM crypto_quote_withdraw($1)",
    )
    .bind(rub_minor)
    .fetch_one(pool)
    .await
    .map_err(CryptoError::from_db)
}

/// Assign or return the user's unique TRC-20 deposit address.
pub async fn ensure_deposit_address(
    pool: &PgPool,
    user_id: Uuid,
    seed: &[u8; 32],
) -> Result<DepositAddress, CryptoError> {
    if let Some(existing) = sqlx::query_as::<_, DepositAddress>(
        r#"
        SELECT id, user_id, address, derivation_index
          FROM crypto_deposit_addresses
         WHERE user_id = $1 AND chain = 'tron'
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(CryptoError::from_db)?
    {
        return Ok(existing);
    }

    let mut tx = pool.begin().await.map_err(CryptoError::from_db)?;
    let index: i32 = sqlx::query_scalar("SELECT nextval('crypto_deposit_index_seq')::INT")
        .fetch_one(&mut *tx)
        .await
        .map_err(CryptoError::from_db)?;

    let key = derive_deposit_address(seed, index as u32)
        .map_err(|e| CryptoError::msg(e.to_string()))?;
    let row = sqlx::query_as::<_, DepositAddress>(
        r#"
        INSERT INTO crypto_deposit_addresses (user_id, chain, address, derivation_index)
        VALUES ($1, 'tron', $2, $3)
        RETURNING id, user_id, address, derivation_index
        "#,
    )
    .bind(user_id)
    .bind(&key.address)
    .bind(index)
    .fetch_one(&mut *tx)
    .await
    .map_err(CryptoError::from_db)?;
    tx.commit().await.map_err(CryptoError::from_db)?;
    Ok(row)
}

pub async fn list_deposits(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
) -> Result<Vec<DepositRow>, CryptoError> {
    sqlx::query_as::<_, DepositRow>(
        r#"
        SELECT id, tx_hash, usdt_minor, rub_credit_minor, fee_minor, status, detected_at
          FROM crypto_deposits
         WHERE user_id = $1
         ORDER BY detected_at DESC
         LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
    .map_err(CryptoError::from_db)
}

pub async fn list_withdrawals(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
) -> Result<Vec<WithdrawRow>, CryptoError> {
    sqlx::query_as::<_, WithdrawRow>(
        r#"
        SELECT id, to_address, rub_debit_minor, usdt_out_minor, fee_minor,
               status, tx_hash, created_at
          FROM crypto_withdrawals
         WHERE user_id = $1
         ORDER BY created_at DESC
         LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
    .map_err(CryptoError::from_db)
}

pub async fn queue_withdraw(
    pool: &PgPool,
    user_id: Uuid,
    rub_minor: i64,
    to_address: &str,
    idempotency_key: &str,
) -> Result<WithdrawResult, CryptoError> {
    // Ensure RUB checking exists.
    crate::ledger::ensure_checking(pool, user_id, "RUB", 0)
        .await
        .map_err(|e| CryptoError::msg(e.to_string()))?;

    sqlx::query_as::<_, WithdrawResult>(
        "SELECT * FROM crypto_queue_withdraw($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(rub_minor)
    .bind(to_address)
    .bind(idempotency_key)
    .fetch_one(pool)
    .await
    .map_err(CryptoError::from_db)
}

pub async fn credit_deposit(
    pool: &PgPool,
    user_id: Uuid,
    tx_hash: &str,
    from_address: Option<&str>,
    to_address: &str,
    usdt_minor: i64,
    block_ts: Option<DateTime<Utc>>,
) -> Result<(Uuid, i64, i64, bool), CryptoError> {
    #[derive(sqlx::FromRow)]
    struct Row {
        deposit_id: Uuid,
        journal_entry_id: Option<Uuid>,
        rub_credit_minor: i64,
        fee_minor: i64,
        already_credited: bool,
    }
    let row = sqlx::query_as::<_, Row>(
        "SELECT * FROM crypto_credit_deposit($1, $2, $3, $4, $5, $6)",
    )
    .bind(user_id)
    .bind(tx_hash)
    .bind(from_address)
    .bind(to_address)
    .bind(usdt_minor)
    .bind(block_ts)
    .fetch_one(pool)
    .await
    .map_err(CryptoError::from_db)?;
    let _ = row.journal_entry_id;
    Ok((
        row.deposit_id,
        row.rub_credit_minor,
        row.fee_minor,
        row.already_credited,
    ))
}

pub async fn list_watched_addresses(
    pool: &PgPool,
) -> Result<Vec<(Uuid, String)>, CryptoError> {
    sqlx::query_as::<_, (Uuid, String)>(
        "SELECT user_id, address FROM crypto_deposit_addresses WHERE chain = 'tron'",
    )
    .fetch_all(pool)
    .await
    .map_err(CryptoError::from_db)
}

/// Atomically claim queued withdrawals (`queued` → `sending`) so only one
/// worker may broadcast each row. Returns `(id, to_address, usdt_out_minor)`.
pub async fn claim_queued_withdrawals(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<(Uuid, String, i64)>, CryptoError> {
    sqlx::query_as::<_, (Uuid, String, i64)>(
        r#"
        SELECT id, to_address, usdt_out_minor
          FROM crypto_claim_next_withdrawals($1)
        "#,
    )
    .bind(limit as i32)
    .fetch_all(pool)
    .await
    .map_err(CryptoError::from_db)
}
