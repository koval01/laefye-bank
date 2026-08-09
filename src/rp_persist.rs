//! Persist / restore Relying Party origin across API process restarts.

use passkey_rs::RelyingParty;
use sqlx::PgPool;

const RP_ORIGIN_KEY: &str = "rp_origin";
const RP_ID_KEY: &str = "rp_id";
const RP_NAME_KEY: &str = "rp_name";

pub async fn save_rp(pool: &PgPool, rp: &RelyingParty) -> Result<(), String> {
    for (key, value) in [
        (RP_ORIGIN_KEY, rp.origin.as_str()),
        (RP_ID_KEY, rp.id.as_str()),
        (RP_NAME_KEY, rp.name.as_str()),
    ] {
        sqlx::query(
            r#"
            INSERT INTO runtime_kv(key, value, updated_at)
            VALUES ($1, $2, now())
            ON CONFLICT (key) DO UPDATE
              SET value = EXCLUDED.value,
                  updated_at = now()
            "#,
        )
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn load_rp(pool: &PgPool) -> Result<Option<RelyingParty>, String> {
    let origin: Option<String> = sqlx::query_scalar(
        "SELECT value FROM runtime_kv WHERE key = $1",
    )
    .bind(RP_ORIGIN_KEY)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    let Some(origin) = origin.filter(|o| !o.is_empty() && !o.contains("pending.tunnel")) else {
        return Ok(None);
    };

    let rp_id: String = sqlx::query_scalar("SELECT value FROM runtime_kv WHERE key = $1")
        .bind(RP_ID_KEY)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| {
            url::Url::parse(&origin)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| origin.clone())
        });

    let name: String = sqlx::query_scalar("SELECT value FROM runtime_kv WHERE key = $1")
        .bind(RP_NAME_KEY)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "Laefye Bank".into());

    Ok(Some(RelyingParty::new(rp_id, name, origin)))
}
