//! Document Security Module — TSE-inspired seals for bank PDFs.
//!
//! Mirrors German TSE / DSFinV-K receipt fields (serial, process type/data,
//! transaction number, signature counter, timestamps, signature, public key)
//! with an Ed25519 software SE. Not a certified BSI hardware TSE.

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

const DSM_SEED_KV: &str = "dsm_ed25519_seed_hex";
const SIG_ALG: &str = "ed25519";
const LOG_TIME: &str = "unixTime";

#[derive(Debug, Clone)]
pub struct DocumentSeal {
    pub serial: String,
    pub process_type: String,
    pub process_data: String,
    pub transaction_number: i64,
    pub signature_counter: i64,
    pub signed_at: DateTime<Utc>,
    pub signature_b64: String,
    pub previous_signature_b64: String,
    pub public_key_b64: String,
    pub qr_payload: String,
    pub sig_alg: &'static str,
}

#[derive(Clone)]
pub struct DocumentSecurityModule {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    serial: String,
    public_key_b64: String,
    ledger: PgPool,
}

impl DocumentSecurityModule {
    pub async fn boot(auth_db: &PgPool, ledger: PgPool) -> Result<Arc<Self>, String> {
        let seed = load_or_create_seed(auth_db).await?;
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        let pk = verifying_key.to_bytes();
        let public_key_b64 = B64.encode(pk);
        let serial = format!(
            "LAEFYE-DSM-{}",
            &hex::encode(Sha256::digest(pk))[..16].to_uppercase()
        );

        sqlx::query(
            r#"
            INSERT INTO document_dsm (id, signature_counter, last_signature_b64, serial, public_key_b64)
            VALUES (1, 0, '', $1, $2)
            ON CONFLICT (id) DO UPDATE
              SET serial = EXCLUDED.serial,
                  public_key_b64 = EXCLUDED.public_key_b64,
                  updated_at = now()
            "#,
        )
        .bind(&serial)
        .bind(&public_key_b64)
        .execute(&ledger)
        .await
        .map_err(|e| format!("dsm bootstrap: {e}"))?;

        tracing::info!(%serial, "document security module (TSE-inspired) ready");

        Ok(Arc::new(Self {
            signing_key,
            verifying_key,
            serial,
            public_key_b64,
            ledger,
        }))
    }

    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn public_key_b64(&self) -> &str {
        &self.public_key_b64
    }

    pub fn sig_alg(&self) -> &'static str {
        SIG_ALG
    }

    /// Seal a document once; reuses an existing seal for the same `(doc_type, doc_ref)`.
    pub async fn seal_or_get(
        &self,
        doc_type: &str,
        doc_ref: &str,
        process_type: &str,
        process_data: &str,
    ) -> Result<DocumentSeal, String> {
        if let Some(existing) = self.get_seal(doc_type, doc_ref).await? {
            return Ok(existing);
        }
        self.seal_new(doc_type, doc_ref, process_type, process_data)
            .await
    }

    pub async fn get_seal(
        &self,
        doc_type: &str,
        doc_ref: &str,
    ) -> Result<Option<DocumentSeal>, String> {
        let row = sqlx::query_as::<_, SealRow>(
            r#"
            SELECT transaction_number, signature_counter, process_type, process_data,
                   signed_at, signature_b64, previous_signature_b64, qr_payload
            FROM document_seals
            WHERE doc_type = $1 AND doc_ref = $2
            "#,
        )
        .bind(doc_type)
        .bind(doc_ref)
        .fetch_optional(&self.ledger)
        .await
        .map_err(|e| e.to_string())?;

        Ok(row.map(|r| DocumentSeal {
            serial: self.serial.clone(),
            process_type: r.process_type,
            process_data: r.process_data,
            transaction_number: r.transaction_number,
            signature_counter: r.signature_counter,
            signed_at: r.signed_at,
            signature_b64: r.signature_b64,
            previous_signature_b64: r.previous_signature_b64,
            public_key_b64: self.public_key_b64.clone(),
            qr_payload: r.qr_payload,
            sig_alg: SIG_ALG,
        }))
    }

    async fn seal_new(
        &self,
        doc_type: &str,
        doc_ref: &str,
        process_type: &str,
        process_data: &str,
    ) -> Result<DocumentSeal, String> {
        let mut tx = self
            .ledger
            .begin()
            .await
            .map_err(|e| format!("dsm begin: {e}"))?;

        let (counter, prev_sig): (i64, String) = sqlx::query_as(
            r#"
            SELECT signature_counter, last_signature_b64
            FROM document_dsm
            WHERE id = 1
            FOR UPDATE
            "#,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| format!("dsm lock: {e}"))?;

        let next_counter = counter + 1;
        let signed_at = Utc::now();
        let start = signed_at;
        let end = signed_at;

        let message = canonical_message(
            &self.serial,
            process_type,
            process_data,
            next_counter,
            next_counter,
            &start,
            &end,
            &prev_sig,
        );
        let signature: Signature = self.signing_key.sign(message.as_bytes());
        let signature_b64 = B64.encode(signature.to_bytes());

        let qr_payload = format!(
            "V0;{};{};{};{};{};{};{};{};{};{};{}",
            self.serial,
            process_type,
            escape_semicolons(process_data),
            next_counter,
            next_counter,
            rfc3339(start),
            rfc3339(end),
            SIG_ALG,
            LOG_TIME,
            signature_b64,
            self.public_key_b64,
        );

        sqlx::query(
            r#"
            UPDATE document_dsm
            SET signature_counter = $1,
                last_signature_b64 = $2,
                updated_at = now()
            WHERE id = 1
            "#,
        )
        .bind(next_counter)
        .bind(&signature_b64)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("dsm counter: {e}"))?;

        let inserted = sqlx::query(
            r#"
            INSERT INTO document_seals (
                doc_type, doc_ref, transaction_number, signature_counter,
                process_type, process_data, signed_at, signature_b64,
                previous_signature_b64, qr_payload
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            ON CONFLICT (doc_type, doc_ref) DO NOTHING
            "#,
        )
        .bind(doc_type)
        .bind(doc_ref)
        .bind(next_counter)
        .bind(next_counter)
        .bind(process_type)
        .bind(process_data)
        .bind(signed_at)
        .bind(&signature_b64)
        .bind(&prev_sig)
        .bind(&qr_payload)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("dsm seal insert: {e}"))?;

        if inserted.rows_affected() == 0 {
            tx.rollback().await.ok();
            return self
                .get_seal(doc_type, doc_ref)
                .await?
                .ok_or_else(|| "dsm seal race: missing".into());
        }

        tx.commit()
            .await
            .map_err(|e| format!("dsm commit: {e}"))?;

        Ok(DocumentSeal {
            serial: self.serial.clone(),
            process_type: process_type.into(),
            process_data: process_data.into(),
            transaction_number: next_counter,
            signature_counter: next_counter,
            signed_at,
            signature_b64,
            previous_signature_b64: prev_sig,
            public_key_b64: self.public_key_b64.clone(),
            qr_payload,
            sig_alg: SIG_ALG,
        })
    }

    pub async fn verify_qr_payload_async(&self, payload: &str) -> Result<VerifyResult, String> {
        let parts: Vec<&str> = payload.split(';').collect();
        if parts.len() != 12 || parts[0] != "V0" {
            return Err("некорректный QR payload".into());
        }
        let process_type = parts[2].to_string();
        let process_data = unescape_semicolons(parts[3]);
        let trans_no: i64 = parts[4]
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let sig_z: i64 = parts[5]
            .parse()
            .map_err(|e: std::num::ParseIntError| e.to_string())?;
        let start = DateTime::parse_from_rfc3339(parts[6])
            .map_err(|e| e.to_string())?
            .with_timezone(&Utc);
        let end = DateTime::parse_from_rfc3339(parts[7])
            .map_err(|e| e.to_string())?
            .with_timezone(&Utc);
        let signature_b64 = parts[10].to_string();
        if parts[1] != self.serial || parts[11] != self.public_key_b64 {
            return Ok(VerifyResult {
                valid: false,
                reason: "ключ/серийник DSM не совпадает".into(),
                transaction_number: Some(trans_no),
                signature_counter: Some(sig_z),
            });
        }

        let prev: String = sqlx::query_scalar(
            r#"
            SELECT previous_signature_b64 FROM document_seals
            WHERE signature_counter = $1
            LIMIT 1
            "#,
        )
        .bind(sig_z)
        .fetch_optional(&self.ledger)
        .await
        .map_err(|e| e.to_string())?
        .unwrap_or_default();

        let message = canonical_message(
            &self.serial,
            &process_type,
            &process_data,
            trans_no,
            sig_z,
            &start,
            &end,
            &prev,
        );
        let sig_bytes = B64
            .decode(&signature_b64)
            .map_err(|e| format!("подпись base64: {e}"))?;
        let sig_arr: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| "подпись должна быть 64 байта".to_string())?;
        let signature = Signature::from_bytes(&sig_arr);
        let valid = self
            .verifying_key
            .verify(message.as_bytes(), &signature)
            .is_ok();

        Ok(VerifyResult {
            valid,
            reason: if valid {
                "подпись DSM действительна".into()
            } else {
                "подпись не сходится".into()
            },
            transaction_number: Some(trans_no),
            signature_counter: Some(sig_z),
        })
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VerifyResult {
    pub valid: bool,
    pub reason: String,
    pub transaction_number: Option<i64>,
    pub signature_counter: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct SealRow {
    transaction_number: i64,
    signature_counter: i64,
    process_type: String,
    process_data: String,
    signed_at: DateTime<Utc>,
    signature_b64: String,
    previous_signature_b64: String,
    qr_payload: String,
}

async fn load_or_create_seed(auth_db: &PgPool) -> Result<[u8; 32], String> {
    if let Ok(hex_env) = std::env::var("LAEFYE_DSM_SEED") {
        return parse_seed_hex(&hex_env);
    }

    let existing: Option<String> =
        sqlx::query_scalar("SELECT value FROM runtime_kv WHERE key = $1")
            .bind(DSM_SEED_KV)
            .fetch_optional(auth_db)
            .await
            .map_err(|e| e.to_string())?;

    if let Some(hex) = existing {
        return parse_seed_hex(&hex);
    }

    let signing_key = SigningKey::generate(&mut OsRng);
    let hex = hex::encode(signing_key.to_bytes());
    sqlx::query(
        r#"
        INSERT INTO runtime_kv(key, value, updated_at)
        VALUES ($1, $2, now())
        ON CONFLICT (key) DO NOTHING
        "#,
    )
    .bind(DSM_SEED_KV)
    .bind(&hex)
    .execute(auth_db)
    .await
    .map_err(|e| e.to_string())?;

    let stored: String = sqlx::query_scalar("SELECT value FROM runtime_kv WHERE key = $1")
        .bind(DSM_SEED_KV)
        .fetch_one(auth_db)
        .await
        .map_err(|e| e.to_string())?;
    parse_seed_hex(&stored)
}

fn parse_seed_hex(s: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(s.trim()).map_err(|e| format!("DSM seed hex: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| "DSM seed must be 32 bytes (64 hex chars)".into())
}

fn rfc3339(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn escape_semicolons(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;")
}

fn unescape_semicolons(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(';') => out.push(';'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn canonical_message(
    serial: &str,
    process_type: &str,
    process_data: &str,
    trans_no: i64,
    sig_z: i64,
    start: &DateTime<Utc>,
    end: &DateTime<Utc>,
    previous_signature_b64: &str,
) -> String {
    format!(
        "LAEFYE-DSM-V1\nserial={serial}\nprocessType={process_type}\nprocessData={process_data}\ntransNo={trans_no}\nsigCounter={sig_z}\nstart={}\nend={}\nprevSig={previous_signature_b64}\n",
        rfc3339(*start),
        rfc3339(*end),
    )
}

/// Build processData for a ledger receipt (Kassenbeleg-like).
pub fn receipt_process_data(
    amount_minor: i64,
    currency: &str,
    entry_id: &str,
    client_handle: &str,
) -> String {
    format!(
        "Beleg^1.0^Betrag:{amount_minor}^Waehrung:{currency}^Vorgang:{entry_id}^Kunde:{client_handle}"
    )
}

/// Build processData for an account statement.
pub fn statement_process_data(account_id: &str, row_count: usize, doc_no: &str) -> String {
    format!("Auszug^1.0^Konto:{account_id}^Zeilen:{row_count}^Dok:{doc_no}")
}
