//! Market data workers: CBR SOAP, Binance, CoinGecko → USDT/RUB mid.

mod binance;
mod cbr;
mod coingecko;
mod worker;

pub use worker::spawn_market_workers;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;

use crate::ledger::LedgerError;

#[derive(Debug, Clone)]
pub struct MarketConfig {
    pub poll_secs: u64,
    pub cbr_soap_url: String,
    pub cbr_xml_url: String,
    pub binance_base: String,
    pub coingecko_base: String,
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            poll_secs: 60,
            cbr_soap_url: "https://www.cbr.ru/DailyInfoWebServ/DailyInfo.asmx".into(),
            cbr_xml_url: "https://www.cbr.ru/scripts/XML_daily.asp".into(),
            binance_base: "https://api.binance.com".into(),
            coingecko_base: "https://api.coingecko.com".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct MarketSnapshot {
    pub usdt_rub_mid: Decimal,
    pub usdt_usd_mid: Decimal,
    pub usdt_rub_source: String,
    pub rates_updated_at: Option<DateTime<Utc>>,
    pub cbr_usd_rub: Option<Decimal>,
    pub cbr_key_rate: Option<Decimal>,
    pub cbr_inflation: Option<Decimal>,
}

pub async fn load_snapshot(pool: &PgPool) -> Result<MarketSnapshot, LedgerError> {
    let row = sqlx::query_as::<_, MarketSnapshot>(
        r#"
        SELECT c.usdt_rub_mid,
               c.usdt_usd_mid,
               c.usdt_rub_source,
               c.rates_updated_at,
               i.usd_rub AS cbr_usd_rub,
               i.key_rate AS cbr_key_rate,
               i.inflation AS cbr_inflation
          FROM crypto_settings c
          CROSS JOIN cbr_indicators i
         WHERE c.id = 1 AND i.id = 1
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(LedgerError::from_db)?;
    Ok(row)
}

pub async fn insert_tick(
    pool: &PgPool,
    source: &str,
    symbol: &str,
    price: Decimal,
    meta: serde_json::Value,
) -> Result<(), LedgerError> {
    sqlx::query(
        r#"
        INSERT INTO market_ticks (source, symbol, price, meta)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(source)
    .bind(symbol)
    .bind(price)
    .bind(meta)
    .execute(pool)
    .await
    .map_err(LedgerError::from_db)?;
    Ok(())
}

pub async fn apply_crypto_mids(
    pool: &PgPool,
    usdt_rub: Decimal,
    usdt_usd: Decimal,
    rub_source: &str,
) -> Result<(), LedgerError> {
    if usdt_rub <= Decimal::ZERO || usdt_usd <= Decimal::ZERO {
        return Err(LedgerError::msg("invalid market mids"));
    }
    sqlx::query(
        r#"
        UPDATE crypto_settings
           SET usdt_rub_mid = $1,
               usdt_usd_mid = $2,
               usdt_rub_source = $3,
               rates_updated_at = NOW()
         WHERE id = 1
        "#,
    )
    .bind(usdt_rub)
    .bind(usdt_usd)
    .bind(rub_source)
    .execute(pool)
    .await
    .map_err(LedgerError::from_db)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_cbr_indicators(
    pool: &PgPool,
    key_rate: Option<Decimal>,
    key_rate_date: Option<chrono::NaiveDate>,
    inflation: Option<Decimal>,
    inflation_date: Option<chrono::NaiveDate>,
    usd_rub: Option<Decimal>,
    eur_rub: Option<Decimal>,
    cny_rub: Option<Decimal>,
    fx_as_of: Option<chrono::NaiveDate>,
    gold_rub: Option<Decimal>,
    reserves_bln_usd: Option<Decimal>,
    ruonia: Option<Decimal>,
    raw: serde_json::Value,
) -> Result<(), LedgerError> {
    sqlx::query(
        r#"
        UPDATE cbr_indicators SET
            key_rate = COALESCE($1, key_rate),
            key_rate_date = COALESCE($2, key_rate_date),
            inflation = COALESCE($3, inflation),
            inflation_date = COALESCE($4, inflation_date),
            usd_rub = COALESCE($5, usd_rub),
            eur_rub = COALESCE($6, eur_rub),
            cny_rub = COALESCE($7, cny_rub),
            fx_as_of = COALESCE($8, fx_as_of),
            gold_rub = COALESCE($9, gold_rub),
            reserves_bln_usd = COALESCE($10, reserves_bln_usd),
            ruonia = COALESCE($11, ruonia),
            raw = $12,
            updated_at = NOW()
         WHERE id = 1
        "#,
    )
    .bind(key_rate)
    .bind(key_rate_date)
    .bind(inflation)
    .bind(inflation_date)
    .bind(usd_rub)
    .bind(eur_rub)
    .bind(cny_rub)
    .bind(fx_as_of)
    .bind(gold_rub)
    .bind(reserves_bln_usd)
    .bind(ruonia)
    .bind(raw)
    .execute(pool)
    .await
    .map_err(LedgerError::from_db)?;
    Ok(())
}
