//! FX settings + CBR ingest. Quotes / margin / spread math live in PostgreSQL.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::str::FromStr;

use crate::ledger::LedgerError;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FxSettings {
    pub margin_bps: i32,
    pub spread_bps: i32,
    pub enabled_currencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Quote {
    pub base: String,
    pub quote: String,
    pub mid: Decimal,
    pub buy: Decimal,
    pub sell: Decimal,
    pub margin_bps: i32,
    pub spread_bps: i32,
    pub as_of: NaiveDate,
}

pub async fn load_settings(pool: &PgPool) -> Result<FxSettings, LedgerError> {
    let row = sqlx::query_as::<_, FxSettings>(
        "SELECT margin_bps, spread_bps, enabled_currencies FROM fx_settings WHERE id = 1",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn update_settings(
    pool: &PgPool,
    margin_bps: i32,
    spread_bps: i32,
    enabled_currencies: Vec<String>,
    updated_by: Option<uuid::Uuid>,
) -> Result<FxSettings, LedgerError> {
    if !(0..=5000).contains(&margin_bps) || !(0..=5000).contains(&spread_bps) {
        return Err(LedgerError::msg("bps out of range 0..=5000"));
    }
    sqlx::query(
        r#"
        UPDATE fx_settings
           SET margin_bps = $1,
               spread_bps = $2,
               enabled_currencies = $3,
               updated_at = NOW(),
               updated_by = $4
         WHERE id = 1
        "#,
    )
    .bind(margin_bps)
    .bind(spread_bps)
    .bind(&enabled_currencies)
    .bind(updated_by)
    .execute(pool)
    .await?;
    load_settings(pool).await
}

/// Mid + buy/sell from SQL engine (`fx_quote_pair`).
pub async fn quote_pair(pool: &PgPool, base: &str, quote: &str) -> Result<Quote, LedgerError> {
    // CHAR(3) comes back padded sometimes — trim in SQL.
    let row = sqlx::query_as::<_, Quote>(
        r#"
        SELECT trim(both FROM base)::text AS base,
               trim(both FROM quote)::text AS quote,
               mid, buy, sell, margin_bps, spread_bps, as_of
          FROM fx_quote_pair($1, $2)
        "#,
    )
    .bind(base.to_ascii_uppercase())
    .bind(quote.to_ascii_uppercase())
    .fetch_one(pool)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("ERROR:") {
            LedgerError::msg(msg.split("ERROR:").last().unwrap_or(&msg).trim())
        } else {
            LedgerError::Db(e)
        }
    })?;
    Ok(row)
}

/// Fetch CBR XML and upsert rates (network I/O stays in the app).
pub async fn refresh_cbr(pool: &PgPool, url: &str) -> Result<usize, LedgerError> {
    let xml = reqwest::get(url)
        .await
        .map_err(|e| LedgerError::msg(e.to_string()))?
        .text()
        .await
        .map_err(|e| LedgerError::msg(e.to_string()))?;

    let (as_of, rates) = parse_cbr_xml(&xml)?;
    let mut n = 0;
    for (code, (nominal, value_rub)) in rates {
        let mid = value_rub / Decimal::from(nominal.max(1));
        sqlx::query(
            r#"
            INSERT INTO fx_rates (base, quote, mid, nominal, as_of, source)
            VALUES ($1, 'RUB', $2, $3, $4, 'cbr')
            ON CONFLICT (base, quote, as_of) DO UPDATE
              SET mid = EXCLUDED.mid,
                  nominal = EXCLUDED.nominal,
                  fetched_at = NOW()
            "#,
        )
        .bind(&code)
        .bind(mid)
        .bind(nominal)
        .bind(as_of)
        .execute(pool)
        .await?;
        n += 1;
    }
    sqlx::query(
        r#"
        INSERT INTO fx_rates (base, quote, mid, nominal, as_of, source)
        VALUES ('RUB', 'RUB', 1, 1, $1, 'cbr')
        ON CONFLICT (base, quote, as_of) DO NOTHING
        "#,
    )
    .bind(as_of)
    .execute(pool)
    .await?;
    Ok(n)
}

fn parse_cbr_xml(xml: &str) -> Result<(NaiveDate, HashMap<String, (i32, Decimal)>), LedgerError> {
    let date_re = regex_lite::Regex::new(r#"Date="(\d{2})\.(\d{2})\.(\d{4})""#)
        .map_err(|e| LedgerError::msg(e.to_string()))?;
    let caps = date_re
        .captures(xml)
        .ok_or_else(|| LedgerError::msg("CBR XML: missing Date"))?;
    let d = caps[1].parse::<u32>().unwrap_or(1);
    let m = caps[2].parse::<u32>().unwrap_or(1);
    let y = caps[3].parse::<i32>().unwrap_or(1970);
    let as_of = NaiveDate::from_ymd_opt(y, m, d).ok_or_else(|| LedgerError::msg("bad CBR date"))?;

    let block_re = regex_lite::Regex::new(
        r#"(?s)<Valute[^>]*>.*?<CharCode>([A-Z]{3})</CharCode>.*?<Nominal>(\d+)</Nominal>.*?<Value>([0-9,]+)</Value>.*?</Valute>"#,
    )
    .map_err(|e| LedgerError::msg(e.to_string()))?;

    let mut map = HashMap::new();
    for caps in block_re.captures_iter(xml) {
        let code = caps[1].to_string();
        let nominal: i32 = caps[2].parse().unwrap_or(1);
        let value_str = caps[3].replace(',', ".");
        let value = Decimal::from_str(&value_str)
            .map_err(|e| LedgerError::msg(format!("CBR value {code}: {e}")))?;
        map.insert(code, (nominal, value));
    }
    if map.is_empty() {
        return Err(LedgerError::msg("CBR XML: no currencies parsed"));
    }
    Ok((as_of, map))
}
