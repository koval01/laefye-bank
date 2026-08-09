//! CoinGecko tether market prices.

use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
struct SimplePrice {
    tether: TetherPx,
}

#[derive(Debug, Deserialize)]
struct TetherPx {
    usd: Option<f64>,
    rub: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct TetherQuote {
    pub usd: Decimal,
    pub rub: Decimal,
}

pub async fn tether_simple(base: &str) -> Result<TetherQuote, String> {
    let url = format!("{base}/api/v3/simple/price?ids=tether&vs_currencies=rub,usd");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header(
            "User-Agent",
            "laefye-bank/0.1 (+https://laefye.bank; market-worker)",
        )
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let body: SimplePrice = resp.json().await.map_err(|e| e.to_string())?;
    let usd = body
        .tether
        .usd
        .ok_or_else(|| "coingecko: missing usd".to_string())?;
    let rub = body
        .tether
        .rub
        .ok_or_else(|| "coingecko: missing rub".to_string())?;
    Ok(TetherQuote {
        usd: Decimal::from_str(&format!("{usd:.8}")).map_err(|e| e.to_string())?,
        rub: Decimal::from_str(&format!("{rub:.8}")).map_err(|e| e.to_string())?,
    })
}
