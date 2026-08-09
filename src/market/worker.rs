//! Periodic market refresh loop.

use std::time::Duration;

use rust_decimal::Decimal;
use sqlx::PgPool;
use tracing::{info, warn};

use super::{
    apply_crypto_mids, binance, cbr, coingecko, insert_tick, MarketConfig,
};

pub fn spawn_market_workers(pool: PgPool, cfg: MarketConfig) {
    let poll_secs = cfg.poll_secs;
    tokio::spawn(async move {
        // Immediate first tick so quotes work without waiting.
        if let Err(e) = refresh_once(&pool, &cfg).await {
            warn!(error = %e, "market refresh initial tick failed");
        }
        loop {
            tokio::time::sleep(Duration::from_secs(cfg.poll_secs.max(30))).await;
            if let Err(e) = refresh_once(&pool, &cfg).await {
                warn!(error = %e, "market refresh tick failed");
            }
        }
    });
    info!(poll_secs, "market workers started (CBR + Binance + CoinGecko)");
}

async fn refresh_once(pool: &PgPool, cfg: &MarketConfig) -> Result<(), String> {
    // 1) CBR official RUB FX + macro indicators
    if let Err(e) = cbr::refresh_cbr_bundle(pool, &cfg.cbr_soap_url, &cfg.cbr_xml_url).await {
        warn!(error = %e, "CBR refresh failed");
    }

    // 2) CoinGecko tether (primary USDT/RUB + USDT/USD)
    let mut usdt_rub: Option<(Decimal, &str)> = None;
    let mut usdt_usd: Option<(Decimal, &str)> = None;

    match coingecko::tether_simple(&cfg.coingecko_base).await {
        Ok(q) => {
            let _ = insert_tick(
                pool,
                "coingecko",
                "USDT/RUB",
                q.rub,
                serde_json::json!({ "usd": q.usd.to_string() }),
            )
            .await;
            let _ = insert_tick(
                pool,
                "coingecko",
                "USDT/USD",
                q.usd,
                serde_json::json!({}),
            )
            .await;
            usdt_rub = Some((q.rub, "coingecko"));
            usdt_usd = Some((q.usd, "coingecko"));
            info!(rub = %q.rub, usd = %q.usd, "CoinGecko tether ok");
        }
        Err(e) => warn!(error = %e, "CoinGecko tether failed"),
    }

    // 3) Binance USDTRUB — only when TRADING (pair is often BREAK; stale ticker ~91 RUB).
    // CoinGecko stays primary; Binance overrides only if within 5% (spot check).
    match binance::live_ticker_price(&cfg.binance_base, "USDTRUB").await {
        Ok(px) => {
            let _ = insert_tick(pool, "binance", "USDTRUB", px, serde_json::json!({})).await;
            if let Some((cg, _)) = usdt_rub {
                let diff = ((px - cg).abs() / cg) * Decimal::from(100);
                if diff <= Decimal::from(5) {
                    usdt_rub = Some((px, "binance"));
                } else {
                    warn!(binance = %px, coingecko = %cg, "Binance USDTRUB diverges — keeping CoinGecko");
                }
            } else {
                usdt_rub = Some((px, "binance"));
            }
            info!(price = %px, "Binance USDTRUB ok");
        }
        Err(e) => warn!(error = %e, "Binance USDTRUB skipped"),
    }

    // 4) Binance USDCUSDT ≈ USDT/USD peg cross-check
    match binance::live_ticker_price(&cfg.binance_base, "USDCUSDT").await {
        Ok(px) => {
            // USDT ≈ 1/USDCUSDT in USD terms when quoting USDC per USDT... 
            // ticker USDCUSDT = how many USDT per 1 USDC ≈ 1.0006 → USDT/USD ≈ 1/px? 
            // Actually USDCUSDT price means 1 USDC = px USDT, so 1 USDT = 1/px USDC ≈ 1 USD.
            let usdt_per_usd = if px > Decimal::ZERO {
                Decimal::ONE / px
            } else {
                Decimal::ONE
            };
            let _ = insert_tick(
                pool,
                "binance",
                "USDCUSDT",
                px,
                serde_json::json!({ "implied_usdt_usd": usdt_per_usd.to_string() }),
            )
            .await;
            if usdt_usd.is_none() {
                usdt_usd = Some((usdt_per_usd, "binance_usdc"));
            }
        }
        Err(e) => warn!(error = %e, "Binance USDCUSDT failed"),
    }

    // 5) Fallback: CBR USD/RUB * USDT/USD
    if usdt_rub.is_none() {
        let cbr_usd: Option<Decimal> = sqlx::query_scalar(
            "SELECT usd_rub FROM cbr_indicators WHERE id = 1",
        )
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .flatten();
        let peg = usdt_usd.map(|(p, _)| p).unwrap_or(Decimal::ONE);
        if let Some(usd_rub) = cbr_usd {
            usdt_rub = Some((peg * usd_rub, "cbr_x_peg"));
        }
    }

    let Some((rub_mid, rub_src)) = usdt_rub else {
        return Err("no USDT/RUB mid available".into());
    };
    let (usd_mid, _) = usdt_usd.unwrap_or((Decimal::ONE, "default"));

    apply_crypto_mids(pool, rub_mid, usd_mid, rub_src)
        .await
        .map_err(|e| e.to_string())?;
    info!(usdt_rub = %rub_mid, usdt_usd = %usd_mid, source = rub_src, "crypto mids applied");
    Ok(())
}
