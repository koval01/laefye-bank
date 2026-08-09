//! In-app notifications (auth DB).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Notification {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub href: Option<String>,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

pub async fn push(
    pool: &PgPool,
    user_id: Uuid,
    kind: &str,
    title: &str,
    body: &str,
    href: Option<&str>,
) -> Result<Notification, sqlx::Error> {
    sqlx::query_as::<_, Notification>(
        r#"
        INSERT INTO notifications (user_id, kind, title, body, href)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, kind, title, body, href, read_at, created_at
        "#,
    )
    .bind(user_id)
    .bind(kind)
    .bind(title)
    .bind(body)
    .bind(href)
    .fetch_one(pool)
    .await
}

pub async fn list(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
) -> Result<Vec<Notification>, sqlx::Error> {
    sqlx::query_as::<_, Notification>(
        r#"
        SELECT id, kind, title, body, href, read_at, created_at
          FROM notifications
         WHERE user_id = $1
         ORDER BY created_at DESC
         LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
}

pub async fn unread_count(pool: &PgPool, user_id: Uuid) -> Result<i64, sqlx::Error> {
    let (n,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::bigint
          FROM notifications
         WHERE user_id = $1 AND read_at IS NULL
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(n)
}

pub async fn mark_read(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query(
        r#"
        UPDATE notifications
           SET read_at = NOW()
         WHERE id = $1 AND user_id = $2 AND read_at IS NULL
        "#,
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn mark_all_read(pool: &PgPool, user_id: Uuid) -> Result<u64, sqlx::Error> {
    let r = sqlx::query(
        r#"
        UPDATE notifications
           SET read_at = NOW()
         WHERE user_id = $1 AND read_at IS NULL
        "#,
    )
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}
