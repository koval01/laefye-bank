//! KYC: encrypt-then-store. Plaintext never hits object storage or logs.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use aws_credential_types::Credentials;
use aws_sdk_s3::config::{BehaviorVersion, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum KycError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    S3(#[from] aws_sdk_s3::Error),
    #[error("crypto: {0}")]
    Crypto(String),
}

impl KycError {
    fn msg(s: impl Into<String>) -> Self {
        Self::Message(s.into())
    }
}

#[derive(Clone)]
pub struct KycStore {
    client: Client,
    bucket: String,
    key: [u8; 32],
}

impl KycStore {
    pub async fn connect(
        endpoint: &str,
        region: &str,
        bucket: &str,
        access_key: &str,
        secret_key: &str,
        master_key: [u8; 32],
    ) -> Result<Self, KycError> {
        let creds = Credentials::new(access_key, secret_key, None, None, "laefye");
        let conf = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region.to_string()))
            .endpoint_url(endpoint)
            .credentials_provider(creds)
            .force_path_style(true)
            .build();
        let client = Client::from_conf(conf);

        // Ensure bucket exists (demo).
        let _ = client.create_bucket().bucket(bucket).send().await;

        Ok(Self {
            client,
            bucket: bucket.to_string(),
            key: master_key,
        })
    }

    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, KycError> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| KycError::Crypto(e.to_string()))?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let mut out = nonce_bytes.to_vec();
        let ct = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| KycError::Crypto(e.to_string()))?;
        out.extend(ct);
        Ok(out)
    }

    fn decrypt(&self, blob: &[u8]) -> Result<Vec<u8>, KycError> {
        if blob.len() < 13 {
            return Err(KycError::Crypto("ciphertext too short".into()));
        }
        let (nonce_bytes, ct) = blob.split_at(12);
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| KycError::Crypto(e.to_string()))?;
        let nonce = Nonce::from_slice(nonce_bytes);
        cipher
            .decrypt(nonce, ct)
            .map_err(|e| KycError::Crypto(e.to_string()))
    }

    pub async fn put_encrypted(&self, object_key: &str, plaintext: &[u8]) -> Result<(), KycError> {
        let blob = self.encrypt(plaintext)?;
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(object_key)
            .body(ByteStream::from(blob))
            .content_type("application/octet-stream")
            .send()
            .await
            .map_err(aws_sdk_s3::Error::from)?;
        Ok(())
    }

    pub async fn get_decrypted(&self, object_key: &str) -> Result<Vec<u8>, KycError> {
        let obj = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(object_key)
            .send()
            .await
            .map_err(aws_sdk_s3::Error::from)?;
        let bytes = obj
            .body
            .collect()
            .await
            .map_err(|e| KycError::msg(e.to_string()))?
            .into_bytes();
        self.decrypt(&bytes)
    }
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct KycCase {
    pub id: Uuid,
    pub user_id: Uuid,
    pub status: String,
    pub reviewer_note: String,
}

pub async fn get_or_create_draft(pool: &PgPool, user_id: Uuid) -> Result<KycCase, KycError> {
    if let Some(c) = sqlx::query_as::<_, KycCase>(
        r#"
        SELECT id, user_id, status, reviewer_note FROM kyc_cases
         WHERE user_id = $1 AND status IN ('draft','pending','resubmit','approved','rejected')
         ORDER BY created_at DESC LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok(c);
    }
    sqlx::query_as::<_, KycCase>(
        r#"
        INSERT INTO kyc_cases (user_id, status)
        VALUES ($1, 'draft')
        RETURNING id, user_id, status, reviewer_note
        "#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

pub async fn audit(
    pool: &PgPool,
    actor: Option<Uuid>,
    action: &str,
    case_id: Option<Uuid>,
    document_id: Option<Uuid>,
    detail: &str,
) -> Result<(), KycError> {
    // Never put document contents or object keys with PII narratives in detail.
    sqlx::query(
        r#"
        INSERT INTO kyc_audit (actor_user_id, action, case_id, document_id, detail)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(actor)
    .bind(action)
    .bind(case_id)
    .bind(document_id)
    .bind(detail)
    .execute(pool)
    .await?;
    Ok(())
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().into()
}

pub async fn add_document(
    pool: &PgPool,
    store: &KycStore,
    user_id: Uuid,
    case_id: Uuid,
    doc_type: &str,
    content_type: &str,
    plaintext: &[u8],
) -> Result<Uuid, KycError> {
    if plaintext.len() > 10 * 1024 * 1024 {
        return Err(KycError::msg("Файл слишком большой (макс. 10 МБ)"));
    }
    let allowed = ["passport", "selfie", "address_proof", "other"];
    if !allowed.contains(&doc_type) {
        return Err(KycError::msg("Неверный тип документа"));
    }
    let object_key = format!("kyc/{}/{}.bin", user_id, Uuid::new_v4());
    let hash = sha256(plaintext);
    store.put_encrypted(&object_key, plaintext).await?;

    let id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO kyc_documents (
            case_id, user_id, doc_type, object_key, content_type, byte_size, plaintext_sha256
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id
        "#,
    )
    .bind(case_id)
    .bind(user_id)
    .bind(doc_type)
    .bind(&object_key)
    .bind(content_type)
    .bind(plaintext.len() as i32)
    .bind(hash.as_slice())
    .fetch_one(pool)
    .await?;

    audit(
        pool,
        Some(user_id),
        "document.upload",
        Some(case_id),
        Some(id),
        doc_type,
    )
    .await?;
    Ok(id)
}

pub async fn submit_case(pool: &PgPool, user_id: Uuid, case_id: Uuid) -> Result<KycCase, KycError> {
    let docs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM kyc_documents WHERE case_id = $1 AND user_id = $2",
    )
    .bind(case_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    if docs < 1 {
        return Err(KycError::msg("Загрузите хотя бы один документ"));
    }
    let c = sqlx::query_as::<_, KycCase>(
        r#"
        UPDATE kyc_cases
           SET status = 'pending', updated_at = NOW()
         WHERE id = $1 AND user_id = $2 AND status IN ('draft','resubmit')
        RETURNING id, user_id, status, reviewer_note
        "#,
    )
    .bind(case_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| KycError::msg("Заявку нельзя отправить в текущем статусе"))?;
    audit(pool, Some(user_id), "case.submit", Some(case_id), None, "").await?;
    Ok(c)
}

pub async fn set_status(
    pool: &PgPool,
    case_id: Uuid,
    status: &str,
    note: &str,
    reviewer: Uuid,
) -> Result<KycCase, KycError> {
    let allowed = ["approved", "rejected", "resubmit", "pending"];
    if !allowed.contains(&status) {
        return Err(KycError::msg("bad status"));
    }
    let c = sqlx::query_as::<_, KycCase>(
        r#"
        UPDATE kyc_cases
           SET status = $2,
               reviewer_note = $3,
               reviewed_by = $4,
               reviewed_at = NOW(),
               updated_at = NOW()
         WHERE id = $1
        RETURNING id, user_id, status, reviewer_note
        "#,
    )
    .bind(case_id)
    .bind(status)
    .bind(note)
    .bind(reviewer)
    .fetch_one(pool)
    .await?;
    audit(
        pool,
        Some(reviewer),
        &format!("case.{status}"),
        Some(case_id),
        None,
        note,
    )
    .await?;
    Ok(c)
}
