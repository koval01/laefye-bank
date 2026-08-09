//! Binance public ticker prices.

use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
struct TickerPrice {
    symbol: String,
    price: String,
}

#[derive(Debug, Deserialize)]
struct ExchangeInfo {
    symbols: Vec<ExchangeSymbol>,
}

#[derive(Debug, Deserialize)]
struct ExchangeSymbol {
    symbol: String,
    status: String,
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// Returns `Ok(status)` e.g. `TRADING` / `BREAK`. Missing symbol → error.
pub async fn symbol_status(base: &str, symbol: &str) -> Result<String, String> {
    let url = format!("{base}/api/v3/exchangeInfo?symbol={symbol}");
    let resp = client()
        .get(&url)
        .header("User-Agent", "laefye-bank/0.1")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let body: ExchangeInfo = resp.json().await.map_err(|e| e.to_string())?;
    body.symbols
        .into_iter()
        .find(|s| s.symbol == symbol)
        .map(|s| s.status)
        .ok_or_else(|| format!("Binance symbol {symbol} not found"))
}

pub async fn ticker_price(base: &str, symbol: &str) -> Result<Decimal, String> {
    let url = format!("{base}/api/v3/ticker/price?symbol={symbol}");
    let resp = client()
        .get(&url)
        .header("User-Agent", "laefye-bank/0.1")
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let body: TickerPrice = resp.json().await.map_err(|e| e.to_string())?;
    Decimal::from_str(&body.price).map_err(|e| format!("{} {}: {e}", body.symbol, e))
}

/// Live mid only when the pair is actively trading (skip BREAK / HALT / etc.).
pub async fn live_ticker_price(base: &str, symbol: &str) -> Result<Decimal, String> {
    let status = symbol_status(base, symbol).await?;
    if status != "TRADING" {
        return Err(format!("Binance {symbol} status={status} (not TRADING)"));
    }
    ticker_price(base, symbol).await
}
