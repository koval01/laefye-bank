//! TronGrid HTTP client for TRC-20 USDT.

use chrono::{DateTime, TimeZone, Utc};
use secp256k1::ecdsa::RecoverableSignature;
use secp256k1::{Message, Secp256k1, SecretKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::address::TronKey;

#[derive(Debug, Error)]
pub enum TronError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

#[derive(Clone)]
pub struct TronGrid {
    http: reqwest::Client,
    base: String,
    api_key: Option<String>,
    usdt_contract: String,
}

#[derive(Debug, Clone)]
pub struct Trc20Transfer {
    pub tx_hash: String,
    pub from: String,
    pub to: String,
    pub value_minor: i64,
    pub block_ts: Option<DateTime<Utc>>,
}

impl TronGrid {
    pub fn new(base: &str, api_key: Option<String>, usdt_contract: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.trim_end_matches('/').to_string(),
            api_key,
            usdt_contract: usdt_contract.to_string(),
        }
    }

    fn apply_key(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(k) = &self.api_key {
            req.header("TRON-PRO-API-KEY", k)
        } else {
            req
        }
    }

    pub async fn list_trc20_to(
        &self,
        address: &str,
        limit: u32,
    ) -> Result<Vec<Trc20Transfer>, TronError> {
        let url = format!(
            "{}/v1/accounts/{}/transactions/trc20?only_to=true&limit={}&contract_address={}",
            self.base, address, limit, self.usdt_contract
        );
        let req = self.apply_key(self.http.get(&url));
        let resp: Trc20Resp = req.send().await?.error_for_status()?.json().await?;
        let mut out = Vec::new();
        for t in resp.data.unwrap_or_default() {
            let Ok(value) = t.value.parse::<i64>() else {
                continue;
            };
            if value <= 0 {
                continue;
            }
            let block_ts = t
                .block_timestamp
                .and_then(|ms| Utc.timestamp_millis_opt(ms).single());
            out.push(Trc20Transfer {
                tx_hash: t.transaction_id,
                from: t.from,
                to: t.to,
                value_minor: value,
                block_ts,
            });
        }
        Ok(out)
    }

    /// Transfer USDT TRC-20 from hot wallet via triggersmartcontract + local sign.
    pub async fn transfer_usdt(
        &self,
        hot: &TronKey,
        to_address: &str,
        usdt_minor: i64,
    ) -> Result<String, TronError> {
        if usdt_minor <= 0 {
            return Err(TronError::Message("amount must be positive".into()));
        }
        let parameter = encode_trc20_transfer(to_address, usdt_minor)?;
        let body = serde_json::json!({
            "owner_address": hot.address,
            "contract_address": self.usdt_contract,
            "function_selector": "transfer(address,uint256)",
            "parameter": parameter,
            "fee_limit": 100_000_000i64,
            "call_value": 0,
            "visible": true
        });
        let url = format!("{}/wallet/triggersmartcontract", self.base);
        let req = self.apply_key(self.http.post(&url).json(&body));
        let triggered: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
        let mut tx = triggered.get("transaction").cloned().ok_or_else(|| {
            TronError::Message(format!("triggersmartcontract failed: {triggered}"))
        })?;

        let raw_data_hex = tx
            .get("raw_data_hex")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TronError::Message("missing raw_data_hex".into()))?
            .to_string();
        let sig = sign_tron_tx(&raw_data_hex, &hot.private_key)?;
        tx.as_object_mut()
            .ok_or_else(|| TronError::Message("tx not object".into()))?
            .insert("signature".into(), serde_json::json!([hex::encode(sig)]));

        let broadcast_url = format!("{}/wallet/broadcasttransaction", self.base);
        let req = self.apply_key(self.http.post(&broadcast_url).json(&tx));
        let broadcast: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
        if broadcast.get("result").and_then(|v| v.as_bool()) != Some(true) {
            return Err(TronError::Message(format!("broadcast failed: {broadcast}")));
        }
        broadcast
            .get("txid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| TronError::Message(format!("no txid: {broadcast}")))
    }
}

#[derive(Debug, Deserialize)]
struct Trc20Resp {
    data: Option<Vec<Trc20Item>>,
}

#[derive(Debug, Deserialize)]
struct Trc20Item {
    transaction_id: String,
    from: String,
    to: String,
    value: String,
    block_timestamp: Option<i64>,
}

fn encode_trc20_transfer(to_base58: &str, amount: i64) -> Result<String, TronError> {
    let raw = bs58::decode(to_base58)
        .into_vec()
        .map_err(|e| TronError::Message(e.to_string()))?;
    if raw.len() < 21 || raw[0] != 0x41 {
        return Err(TronError::Message("bad to address".into()));
    }
    let addr20 = &raw[1..21];
    let mut param = vec![0u8; 32];
    param[12..32].copy_from_slice(addr20);
    let mut amount_param = [0u8; 32];
    let be = (amount as u64).to_be_bytes();
    amount_param[24..].copy_from_slice(&be);
    param.extend_from_slice(&amount_param);
    Ok(hex::encode(param))
}

fn sign_tron_tx(raw_data_hex: &str, private_key: &[u8; 32]) -> Result<Vec<u8>, TronError> {
    let raw = hex::decode(raw_data_hex).map_err(|e| TronError::Message(e.to_string()))?;
    let hash = Sha256::digest(&raw);
    let secp = Secp256k1::new();
    let sk =
        SecretKey::from_slice(private_key).map_err(|e| TronError::Message(e.to_string()))?;
    let msg =
        Message::from_digest_slice(&hash).map_err(|e| TronError::Message(e.to_string()))?;
    let rec_sig: RecoverableSignature = secp.sign_ecdsa_recoverable(&msg, &sk);
    let (rec_id, compact) = rec_sig.serialize_compact();
    let mut out = Vec::with_capacity(65);
    out.extend_from_slice(&compact);
    out.push(rec_id.to_i32() as u8);
    Ok(out)
}
