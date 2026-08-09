//! Background deposit watcher + withdrawal broadcaster.

use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tracing::{info, warn};

use super::address::hot_wallet_address;
use super::trongrid::TronGrid;
use super::{credit_deposit, list_watched_addresses, CryptoConfig, CryptoMode};

pub fn spawn_crypto_workers(pool: PgPool, cfg: CryptoConfig) {
    let cfg = Arc::new(cfg);
    let pool_watch = pool.clone();
    let cfg_watch = cfg.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = watch_deposits_once(&pool_watch, &cfg_watch).await {
                warn!(error = %e, "crypto deposit watch tick failed");
            }
            tokio::time::sleep(Duration::from_secs(cfg_watch.poll_secs.max(5))).await;
        }
    });

    let pool_wd = pool;
    let cfg_wd = cfg.clone();
    let mode_log = cfg.mode;
    tokio::spawn(async move {
        loop {
            if let Err(e) = process_withdrawals_once(&pool_wd, &cfg_wd).await {
                warn!(error = %e, "crypto withdraw tick failed");
            }
            tokio::time::sleep(Duration::from_secs(cfg_wd.poll_secs.max(5))).await;
        }
    });
    info!(mode = ?mode_log, "USDT TRC-20 workers started");
}

async fn watch_deposits_once(pool: &PgPool, cfg: &CryptoConfig) -> Result<(), String> {
    if cfg.mode != CryptoMode::Mainnet {
        return Ok(());
    }
    let settings = super::load_settings(pool)
        .await
        .map_err(|e| e.to_string())?;
    if !settings.enabled {
        return Ok(());
    }
    let client = TronGrid::new(
        &cfg.trongrid_base,
        cfg.trongrid_api_key.clone(),
        &settings.usdt_contract,
    );
    let addrs = list_watched_addresses(pool)
        .await
        .map_err(|e| e.to_string())?;
    for (user_id, address) in addrs {
        match client.list_trc20_to(&address, 30).await {
            Ok(txs) => {
                for tx in txs {
                    match credit_deposit(
                        pool,
                        user_id,
                        &tx.tx_hash,
                        Some(&tx.from),
                        &tx.to,
                        tx.value_minor,
                        tx.block_ts,
                    )
                    .await
                    {
                        Ok((_, credit, fee, replay)) => {
                            if !replay {
                                info!(
                                    %user_id,
                                    tx = %tx.tx_hash,
                                    usdt = tx.value_minor,
                                    credit,
                                    fee,
                                    "credited USDT deposit"
                                );
                            }
                        }
                        Err(e) => warn!(error = %e, tx = %tx.tx_hash, "credit deposit failed"),
                    }
                }
            }
            Err(e) => warn!(%address, error = %e, "trongrid list failed"),
        }
    }
    Ok(())
}

async fn process_withdrawals_once(pool: &PgPool, cfg: &CryptoConfig) -> Result<(), String> {
    let settings = super::load_settings(pool)
        .await
        .map_err(|e| e.to_string())?;
    if !settings.enabled {
        return Ok(());
    }

    // Claim under one transaction: queued → sending. Never broadcast while still queued.
    let rows = super::claim_queued_withdrawals(pool, 5)
        .await
        .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Ok(());
    }

    match cfg.mode {
        CryptoMode::Mock => {
            for (id, _to, amount) in rows {
                let fake = format!("mock-{}", id.simple());
                sqlx::query("SELECT crypto_mark_withdraw_broadcast($1, $2)")
                    .bind(id)
                    .bind(&fake)
                    .execute(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                sqlx::query("SELECT crypto_mark_withdraw_confirmed($1)")
                    .bind(id)
                    .execute(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                info!(%id, amount, "mock USDT withdraw confirmed");
            }
        }
        CryptoMode::Mainnet => {
            let Some(seed) = cfg.master_seed else {
                warn!("mainnet withdraws queued but TRON_MASTER_SEED unset");
                return Ok(());
            };
            let hot = hot_wallet_address(&seed).map_err(|e| e.to_string())?;
            let client = TronGrid::new(
                &cfg.trongrid_base,
                cfg.trongrid_api_key.clone(),
                &settings.usdt_contract,
            );
            for (id, to, amount) in rows {
                match client.transfer_usdt(&hot, &to, amount).await {
                    Ok(txid) => {
                        sqlx::query("SELECT crypto_mark_withdraw_broadcast($1, $2)")
                            .bind(id)
                            .bind(&txid)
                            .execute(pool)
                            .await
                            .map_err(|e| e.to_string())?;
                        info!(%id, %txid, amount, "broadcast USDT withdraw");
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        warn!(%id, error = %msg, "withdraw broadcast failed");
                        sqlx::query("SELECT crypto_mark_withdraw_failed($1, $2)")
                            .bind(id)
                            .bind(&msg)
                            .execute(pool)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        }
    }
    Ok(())
}
