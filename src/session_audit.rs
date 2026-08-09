//! Session store wrapper that writes auth_events on create/destroy.

use std::sync::Arc;

use async_trait::async_trait;
use passkey_rs::error::PasskeyResult;
use passkey_rs::session::{NewSession, Session, SessionStore};
use sqlx::PgPool;
use uuid::Uuid;

pub struct AuditingSessionStore {
    inner: Arc<dyn SessionStore>,
    auth_db: PgPool,
}

impl AuditingSessionStore {
    pub fn new(inner: Arc<dyn SessionStore>, auth_db: PgPool) -> Self {
        Self { inner, auth_db }
    }
}

#[async_trait]
impl SessionStore for AuditingSessionStore {
    async fn create(&self, new: NewSession) -> PasskeyResult<Session> {
        let username = new.username.clone();
        let ip = new.client_ip.clone().unwrap_or_default();
        let geo = new
            .client_country
            .as_deref()
            .map(|c| format!(" · {c}"))
            .unwrap_or_default();
        let session = self.inner.create(new).await?;
        let _ = crate::account::record_event(
            &self.auth_db,
            session.user_id,
            "login",
            &ip,
            "",
            &format!("Вход · @{username}{geo}"),
        )
        .await;
        let _ = crate::notifications::push(
            &self.auth_db,
            session.user_id,
            "session",
            "Новый вход",
            &format!("Вход в аккаунт @{username}{geo}"),
            Some("/app/account"),
        )
        .await;
        Ok(session)
    }

    async fn get(&self, session_id: &str) -> PasskeyResult<Option<Session>> {
        self.inner.get(session_id).await
    }

    async fn rotate_proof(
        &self,
        session_id: &str,
        expected_proof: &str,
    ) -> PasskeyResult<Option<String>> {
        self.inner.rotate_proof(session_id, expected_proof).await
    }

    async fn grant_step_up(
        &self,
        session_id: &str,
        until: time::OffsetDateTime,
    ) -> PasskeyResult<bool> {
        self.inner.grant_step_up(session_id, until).await
    }

    async fn destroy(&self, session_id: &str) -> PasskeyResult<()> {
        let existing = self.inner.get(session_id).await.ok().flatten();
        self.inner.destroy(session_id).await?;
        if let Some(s) = existing {
            let ip = s.client_ip.as_deref().unwrap_or("");
            let _ = crate::account::record_event(
                &self.auth_db,
                s.user_id,
                "logout",
                ip,
                "",
                "Сессия завершена",
            )
            .await;
        }
        Ok(())
    }

    async fn list_for_user(&self, user_id: Uuid) -> PasskeyResult<Vec<Session>> {
        self.inner.list_for_user(user_id).await
    }

    async fn destroy_all_for_user(
        &self,
        user_id: Uuid,
        except_session_id: Option<&str>,
    ) -> PasskeyResult<usize> {
        let n = self
            .inner
            .destroy_all_for_user(user_id, except_session_id)
            .await?;
        if n > 0 {
            let _ = crate::account::record_event(
                &self.auth_db,
                user_id,
                "session_revoke_others",
                "",
                "",
                &format!("Завершено сессий: {n}"),
            )
            .await;
        }
        Ok(n)
    }
}
