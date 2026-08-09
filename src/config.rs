//! Application configuration.

use std::env;

use passkey_rs::RelyingParty;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub rp: RelyingParty,
    pub http_addr: String,
    pub https_addr: String,
    pub cert_dir: String,
    pub auth_database_url: Option<String>,
    pub ledger_database_url: Option<String>,
    pub redis_url: Option<String>,
    pub welcome_credit_minor: i64,
    pub serve_http_app: bool,
    pub http_only: bool,
    pub block_localhost: bool,
    /// Comma-separated bank handles with admin console access.
    pub admin_handles: Vec<String>,
    /// 32-byte key, base64 — encrypts KYC blobs before object storage.
    pub kyc_master_key: Option<[u8; 32]>,
    pub s3_endpoint: Option<String>,
    pub s3_region: String,
    pub s3_bucket: String,
    pub s3_access_key: Option<String>,
    pub s3_secret_key: Option<String>,
    pub cbr_url: String,
    pub market: crate::market::MarketConfig,
    pub crypto: crate::crypto::CryptoConfig,
    /// `public` (customer edge) or `admin` (corp network only).
    pub app_role: AppRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppRole {
    Public,
    Admin,
}

impl AppRole {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "admin" | "ops" | "backoffice" => Self::Admin,
            _ => Self::Public,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            rp: RelyingParty::new("localhost", "Laefye Bank", "https://localhost:8443"),
            http_addr: "127.0.0.1:8080".into(),
            https_addr: "127.0.0.1:8443".into(),
            cert_dir: "certs".into(),
            auth_database_url: None,
            ledger_database_url: None,
            redis_url: None,
            welcome_credit_minor: 100_000, // ₽1 000
            serve_http_app: false,
            http_only: false,
            block_localhost: false,
            admin_handles: Vec::new(),
            kyc_master_key: None,
            s3_endpoint: None,
            s3_region: "us-east-1".into(),
            s3_bucket: "laefye-kyc".into(),
            s3_access_key: None,
            s3_secret_key: None,
            cbr_url: "https://www.cbr.ru/scripts/XML_daily.asp".into(),
            market: crate::market::MarketConfig::default(),
            crypto: crate::crypto::CryptoConfig::default(),
            app_role: AppRole::Public,
        }
    }
}

fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("yes") | Ok("on")
    )
}

fn parse_key32(raw: &str) -> Option<[u8; 32]> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Some(out)
}

impl AppConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();

        let mut rp_id = cfg.rp.id.clone();
        let mut rp_name = cfg.rp.name.clone();
        let mut origin = cfg.rp.origin.clone();

        if let Ok(v) = env::var("RP_ID") {
            rp_id = v;
        }
        if let Ok(v) = env::var("RP_NAME") {
            rp_name = v;
        }
        if let Ok(v) = env::var("ORIGIN") {
            origin = v;
        }
        if let Ok(v) = env::var("HTTP_ADDR") {
            cfg.http_addr = v;
        }
        if let Ok(v) = env::var("HTTPS_ADDR") {
            cfg.https_addr = v;
        }
        if let Ok(v) = env::var("CERT_DIR") {
            cfg.cert_dir = v;
        }
        if let Ok(v) = env::var("AUTH_DATABASE_URL") {
            if !v.is_empty() {
                cfg.auth_database_url = Some(v);
            }
        }
        if let Ok(v) = env::var("LEDGER_DATABASE_URL") {
            if !v.is_empty() {
                cfg.ledger_database_url = Some(v);
            }
        }
        if let Ok(v) = env::var("DATABASE_URL") {
            if !v.is_empty() {
                if cfg.auth_database_url.is_none() {
                    cfg.auth_database_url = Some(v.clone());
                }
                if cfg.ledger_database_url.is_none() {
                    cfg.ledger_database_url = Some(v);
                }
            }
        }
        if let Ok(v) = env::var("REDIS_URL") {
            if !v.is_empty() {
                cfg.redis_url = Some(v);
            }
        }
        if let Ok(v) = env::var("WELCOME_CREDIT_MINOR") {
            if let Ok(n) = v.parse::<i64>() {
                cfg.welcome_credit_minor = n;
            }
        }
        if let Ok(v) = env::var("ADMIN_HANDLES") {
            cfg.admin_handles = v
                .split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = env::var("KYC_MASTER_KEY") {
            cfg.kyc_master_key = parse_key32(&v);
        }
        if let Ok(v) = env::var("S3_ENDPOINT") {
            if !v.is_empty() {
                cfg.s3_endpoint = Some(v);
            }
        }
        if let Ok(v) = env::var("S3_REGION") {
            cfg.s3_region = v;
        }
        if let Ok(v) = env::var("S3_BUCKET") {
            cfg.s3_bucket = v;
        }
        if let Ok(v) = env::var("S3_ACCESS_KEY") {
            cfg.s3_access_key = Some(v);
        }
        if let Ok(v) = env::var("S3_SECRET_KEY") {
            cfg.s3_secret_key = Some(v);
        }
        if let Ok(v) = env::var("CBR_URL") {
            cfg.cbr_url = v.clone();
            cfg.market.cbr_xml_url = v;
        }
        if let Ok(v) = env::var("CBR_SOAP_URL") {
            if !v.is_empty() {
                cfg.market.cbr_soap_url = v;
            }
        }
        if let Ok(v) = env::var("BINANCE_API") {
            if !v.is_empty() {
                cfg.market.binance_base = v;
            }
        }
        if let Ok(v) = env::var("COINGECKO_API") {
            if !v.is_empty() {
                cfg.market.coingecko_base = v;
            }
        }
        if let Ok(v) = env::var("MARKET_POLL_SECS") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.market.poll_secs = n;
            }
        }
        cfg.crypto.mode = crate::crypto::CryptoMode::parse(
            &env::var("TRON_MODE").unwrap_or_else(|_| "mock".into()),
        );
        if let Ok(v) = env::var("TRONGRID_BASE") {
            if !v.is_empty() {
                cfg.crypto.trongrid_base = v;
            }
        }
        if let Ok(v) = env::var("TRONGRID_API_KEY") {
            if !v.is_empty() {
                cfg.crypto.trongrid_api_key = Some(v);
            }
        }
        if let Ok(v) = env::var("TRON_MASTER_SEED") {
            if let Ok(bytes) = hex::decode(v.trim()) {
                if bytes.len() == 32 {
                    let mut seed = [0u8; 32];
                    seed.copy_from_slice(&bytes);
                    cfg.crypto.master_seed = Some(seed);
                } else {
                    tracing::warn!("TRON_MASTER_SEED must be 32 bytes hex");
                }
            } else {
                tracing::warn!("TRON_MASTER_SEED invalid hex");
            }
        }
        if let Ok(v) = env::var("TRON_POLL_SECS") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.crypto.poll_secs = n.clamp(5, 300);
            }
        }
        cfg.app_role = AppRole::parse(&env::var("APP_ROLE").unwrap_or_else(|_| "public".into()));
        cfg.serve_http_app = env_flag("PASSKEY_SERVE_HTTP");
        cfg.http_only = env_flag("PASSKEY_HTTP_ONLY");
        cfg.block_localhost = env_flag("BLOCK_LOCALHOST");
        if cfg.http_only {
            cfg.serve_http_app = true;
        }

        cfg.rp = RelyingParty::new(rp_id, rp_name, origin);
        cfg
    }
}
