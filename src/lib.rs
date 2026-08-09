//! Laefye Bank — passkeys, double-entry ledger, FX, KYC, admin.

pub mod auth_profile;
pub mod account;
pub mod config;
pub mod crypto;
pub mod documents;
pub mod fx;
pub mod host_gate;
pub mod kyc;
pub mod ledger;
pub mod market;
pub mod migrate;
pub mod notifications;
pub mod pb;
pub mod rp_persist;
pub mod routes;
pub mod secure;
pub mod session_audit;
pub mod session_guard;
pub mod state_boot;
pub mod tls;
pub mod tunnel;

pub use config::AppConfig;

use passkey_rs::axum::PasskeyState;
use sqlx::PgPool;
use std::sync::Arc;

use crate::kyc::KycStore;

#[derive(Clone)]
pub struct AppState {
    pub passkey: PasskeyState,
    pub auth_db: PgPool,
    pub ledger_db: PgPool,
    pub welcome_credit_minor: i64,
    pub admin_handles: Vec<String>,
    pub kyc: Option<Arc<KycStore>>,
    pub cbr_url: String,
    pub crypto: crate::crypto::CryptoConfig,
    pub app_role: crate::config::AppRole,
    pub block_localhost: bool,
    pub secure: Option<crate::secure::ChannelStore>,
    /// TSE-inspired document seals (software DSM).
    pub dsm: Option<Arc<crate::documents::DocumentSecurityModule>>,
}
