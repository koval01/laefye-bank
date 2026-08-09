//! Simple SQL file migrator (ordered by filename).

use sqlx::PgPool;
use std::path::Path;

pub async fn run_sql_dir(pool: &PgPool, dir: impl AsRef<Path>) -> Result<(), String> {
    let dir = dir.as_ref();
    let mut rd = tokio::fs::read_dir(dir)
        .await
        .map_err(|e| format!("read migrations {}: {e}", dir.display()))?;

    let mut files = Vec::new();
    while let Some(e) = rd.next_entry().await.map_err(|e| e.to_string())? {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) == Some("sql") {
            files.push(path);
        }
    }
    files.sort();

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS _laefye_migrations (
            filename TEXT PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    for path in files {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("bad migration path {}", path.display()))?
            .to_string();

        let applied: Option<(String,)> =
            sqlx::query_as("SELECT filename FROM _laefye_migrations WHERE filename = $1")
                .bind(&name)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?;
        if applied.is_some() {
            continue;
        }

        let sql = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| format!("read {}: {e}", path.display()))?;

        let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::raw_sql(&sql)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("apply {name}: {e}"))?;
        sqlx::query("INSERT INTO _laefye_migrations (filename) VALUES ($1)")
            .bind(&name)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        tx.commit().await.map_err(|e| e.to_string())?;
        tracing::info!(%name, dir = %dir.display(), "migration applied");
    }
    Ok(())
}
