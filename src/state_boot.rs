//! Bootstrap PasskeyState + DB pools.

use std::sync::Arc;

use passkey_rs::axum::PasskeyState;
use passkey_rs::config::PasskeyConfig;
use passkey_rs::{
    PostgresUserStore, RedisChallengeStore, RedisRateLimitStore, RedisSessionStore,
};
use sqlx::PgPool;

use crate::config::AppConfig;
use crate::migrate;
use crate::AppState;

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

pub async fn build_app_state(config: &AppConfig) -> Result<AppState, String> {
    let mut pk_config = PasskeyConfig::default();
    // Server-enforced idle: Redis key + validity check. Cookie alone cannot revive.
    pk_config.session_idle_secs = 5 * 60;
    pk_config.session_absolute_secs = 3600;
    pk_config.cookies.session_max_age_secs = pk_config.session_absolute_secs;
    tracing::info!(
        session_idle_secs = pk_config.session_idle_secs,
        session_absolute_secs = pk_config.session_absolute_secs,
        "passkey session TTLs"
    );
    pk_config.allow_open_registration = !env_flag("PASSKEY_DISABLE_OPEN_REGISTRATION");
    if env_flag("PASSKEY_TRUST_FORWARDED") {
        pk_config.trust_forwarded_headers = true;
        tracing::info!("trust_forwarded_headers=true");
    }
    match std::env::var("PASSKEY_ATTESTATION")
        .unwrap_or_else(|_| "compatible".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "none" | "none_only" => {
            pk_config.attestation = passkey_rs::AttestationPolicy::none_only();
        }
        "packed" | "require_packed" | "strict" => {
            pk_config.attestation = passkey_rs::AttestationPolicy::require_packed();
        }
        _ => {
            pk_config.attestation = passkey_rs::AttestationPolicy::compatible();
        }
    }

    let (auth_db, ledger_db, passkey) = match (
        &config.auth_database_url,
        &config.ledger_database_url,
        &config.redis_url,
    ) {
        (Some(auth_url), Some(ledger_url), Some(redis_url)) => {
            tracing::info!("storage → auth Postgres + ledger Postgres + Redis");

            let users = PostgresUserStore::connect(auth_url)
                .await
                .map_err(|e| e.to_string())?;

            // Only the public edge owns schema migrations. Concurrent CREATE from
            // api + admin-api races on pg_type and kills boot.
            let run_migrations = config.app_role == crate::config::AppRole::Public
                && !env_flag("SKIP_MIGRATE");
            if run_migrations {
                users.migrate().await.map_err(|e| e.to_string())?;
            } else {
                tracing::info!("skipping schema migrations (APP_ROLE=admin or SKIP_MIGRATE)");
            }

            let auth_db = PgPool::connect(auth_url)
                .await
                .map_err(|e| e.to_string())?;
            if run_migrations {
                migrate::run_sql_dir(&auth_db, "migrations/auth").await?;
            }

            let ledger_db = crate::ledger::connect(ledger_url)
                .await
                .map_err(|e| e.to_string())?;
            if run_migrations {
                migrate::run_sql_dir(&ledger_db, "migrations/ledger").await?;
            }

            // Best-effort CBR mid rates so USDT↔RUB quotes work without admin click.
            if config.app_role == crate::config::AppRole::Public {
                match crate::fx::refresh_cbr(&ledger_db, &config.cbr_url).await {
                    Ok(n) => tracing::info!(rates = n, "CBR FX rates refreshed"),
                    Err(e) => tracing::warn!(error = %e, "CBR FX refresh skipped"),
                }
            }

            let challenges = Arc::new(
                RedisChallengeStore::connect(redis_url)
                    .await
                    .map_err(|e| e.to_string())?,
            );
            let sessions_inner = Arc::new(
                RedisSessionStore::connect(redis_url)
                    .await
                    .map_err(|e| e.to_string())?,
            );
            let sessions: Arc<dyn passkey_rs::SessionStore> = Arc::new(
                crate::session_audit::AuditingSessionStore::new(sessions_inner, auth_db.clone()),
            );
            let rate_limits = Arc::new(
                RedisRateLimitStore::connect(redis_url)
                    .await
                    .map_err(|e| e.to_string())?,
            );

            let passkey = PasskeyState::with_stores(
                config.rp.clone(),
                Arc::new(users),
                challenges,
                sessions,
                rate_limits,
                pk_config,
            );

            // Restore last tunnel RP after process restart (cloudflared may not re-announce).
            if config.app_role == crate::config::AppRole::Public {
                match crate::rp_persist::load_rp(&auth_db).await {
                    Ok(Some(rp)) => {
                        tracing::info!(origin = %rp.origin, "restored relying party from runtime_kv");
                        passkey.set_relying_party(rp);
                    }
                    Ok(None) => {}
                    Err(e) => tracing::warn!(error = %e, "rp_persist load skipped"),
                }
            }

            (auth_db, ledger_db, passkey)
        }
        _ => {
            return Err(
                "need AUTH_DATABASE_URL, LEDGER_DATABASE_URL, and REDIS_URL (or DATABASE_URL + REDIS_URL)"
                    .into(),
            );
        }
    };

    let kyc = match (
        &config.s3_endpoint,
        &config.s3_access_key,
        &config.s3_secret_key,
        config.kyc_master_key,
    ) {
        (Some(ep), Some(ak), Some(sk), Some(key)) => {
            match crate::kyc::KycStore::connect(
                ep,
                &config.s3_region,
                &config.s3_bucket,
                ak,
                sk,
                key,
            )
            .await
            {
                Ok(store) => {
                    tracing::info!(bucket = %config.s3_bucket, "KYC object store ready");
                    Some(Arc::new(store))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "KYC store unavailable — uploads disabled");
                    None
                }
            }
        }
        _ => {
            tracing::warn!("KYC disabled (set S3_* + KYC_MASTER_KEY)");
            None
        }
    };

    // Persist hot wallet address when seed is configured.
    if let Some(seed) = &config.crypto.master_seed {
        match crate::crypto::hot_wallet_address(seed) {
            Ok(hot) => {
                if let Err(e) =
                    crate::crypto::set_hot_wallet_address(&ledger_db, &hot.address).await
                {
                    tracing::warn!(error = %e, "failed to store hot wallet address");
                } else {
                    tracing::info!(address = %hot.address, "TRON hot wallet ready");
                }
            }
            Err(e) => tracing::warn!(error = %e, "hot wallet derive failed"),
        }
    } else if config.crypto.mode == crate::crypto::CryptoMode::Mainnet {
        tracing::warn!("TRON_MODE=mainnet but TRON_MASTER_SEED unset — deposits watch only if addresses exist; withdraws blocked");
    }

    // Ledger workers (USDT watcher + market rates) only on the public edge.
    if config.app_role == crate::config::AppRole::Public {
        crate::crypto::spawn_crypto_workers(ledger_db.clone(), config.crypto.clone());
        crate::market::spawn_market_workers(ledger_db.clone(), config.market.clone());
    } else {
        tracing::info!("APP_ROLE=admin — crypto/market workers not started on this process");
    }

    // Encrypted customer channel (public edge only).
    let secure = if config.app_role == crate::config::AppRole::Public {
        match &config.redis_url {
            Some(url) => match crate::secure::ChannelStore::connect(url).await {
                Ok(s) => {
                    tracing::info!("secure channel store ready");
                    Some(s)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "secure channel store unavailable");
                    None
                }
            },
            None => None,
        }
    } else {
        None
    };

    let dsm = match crate::documents::DocumentSecurityModule::boot(&auth_db, ledger_db.clone()).await
    {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!(error = %e, "document security module unavailable — PDFs unsigned");
            None
        }
    };

    Ok(AppState {
        passkey,
        auth_db,
        ledger_db,
        welcome_credit_minor: config.welcome_credit_minor,
        admin_handles: config.admin_handles.clone(),
        kyc,
        cbr_url: config.cbr_url.clone(),
        crypto: config.crypto.clone(),
        app_role: config.app_role,
        block_localhost: config.block_localhost,
        secure,
        dsm,
    })
}
