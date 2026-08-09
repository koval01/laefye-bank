//! Redis-backed secure channel sessions.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use super::crypto::SessionKeys;

const CHANNEL_TTL_SECS: u64 = 3600;
const CHALLENGE_TTL_SECS: u64 = 120;

#[derive(Clone)]
pub struct ChannelStore {
    conn: ConnectionManager,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelState {
    pub enc: [u8; 32],
    pub mac: [u8; 32],
    pub counter: u64,
    pub created_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeState {
    pub challenge: Vec<u8>,
    pub difficulty: u32,
}

impl ChannelStore {
    pub async fn connect(redis_url: &str) -> Result<Self, String> {
        let client = redis::Client::open(redis_url).map_err(|e| e.to_string())?;
        let conn = ConnectionManager::new(client)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    pub async fn put_challenge(&self, id: &str, state: &ChallengeState) -> Result<(), String> {
        let mut conn = self.conn.clone();
        let key = format!("sec:chal:{id}");
        let val = serde_json::to_vec(state).map_err(|e| e.to_string())?;
        conn.set_ex::<_, _, ()>(key, val, CHALLENGE_TTL_SECS)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn take_challenge(&self, id: &str) -> Result<Option<ChallengeState>, String> {
        let mut conn = self.conn.clone();
        let key = format!("sec:chal:{id}");
        let val: Option<Vec<u8>> = conn.get_del(key).await.map_err(|e| e.to_string())?;
        match val {
            Some(v) => Ok(Some(serde_json::from_slice(&v).map_err(|e| e.to_string())?)),
            None => Ok(None),
        }
    }

    pub async fn put_channel(&self, id: &str, keys: &SessionKeys) -> Result<(), String> {
        let state = ChannelState {
            enc: keys.enc,
            mac: keys.mac,
            counter: 0,
            created_ms: chrono::Utc::now().timestamp_millis(),
        };
        self.save_channel(id, &state).await
    }

    pub async fn load_channel(&self, id: &str) -> Result<Option<ChannelState>, String> {
        let mut conn = self.conn.clone();
        let key = format!("sec:ch:{id}");
        let val: Option<Vec<u8>> = conn.get(key).await.map_err(|e| e.to_string())?;
        match val {
            Some(v) => Ok(Some(serde_json::from_slice(&v).map_err(|e| e.to_string())?)),
            None => Ok(None),
        }
    }

    pub async fn save_channel(&self, id: &str, state: &ChannelState) -> Result<(), String> {
        let mut conn = self.conn.clone();
        let key = format!("sec:ch:{id}");
        let val = serde_json::to_vec(state).map_err(|e| e.to_string())?;
        conn.set_ex::<_, _, ()>(key, val, CHANNEL_TTL_SECS)
            .await
            .map_err(|e| e.to_string())
    }
}

pub fn channel_ttl_secs() -> u32 {
    CHANNEL_TTL_SECS as u32
}
