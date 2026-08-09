//! Opaque RPC opcodes — scrambled constants, not sequential.
//! Internal handlers still use classic paths via oneshot; the browser never sees them.

use axum::http::Method;

#[derive(Debug, Clone)]
pub struct OpTarget {
    pub method: Method,
    pub path: String,
    pub query: Vec<(String, String)>,
}

/// Resolve an encrypted opcode + args into an internal handler target.
pub fn resolve(op: u32, args: &std::collections::HashMap<String, String>) -> Option<OpTarget> {
    let (method, path_tpl) = match op {
        // Bank / ledger
        0x4A21 => (Method::GET, "/api/bank/me"),
        0x4A22 => (Method::POST, "/api/bank/profile"),
        0x5B01 => (Method::GET, "/api/accounts"),
        0x5B02 => (Method::POST, "/api/accounts/open"),
        0x5B10 => (Method::GET, "/api/accounts/{a}/transactions"),
        0x5B20 => (Method::GET, "/api/transactions/{i}"),
        0x6C01 => (Method::POST, "/api/transfers"),
        0x6C10 => (Method::GET, "/api/fx/quote"),
        0x6C11 => (Method::POST, "/api/fx/convert"),
        0x6C20 => (Method::GET, "/api/market"),
        0x6C30 => (Method::GET, "/api/directory/lookup"),
        // KYC
        0x7D01 => (Method::GET, "/api/kyc"),
        0x7D02 => (Method::POST, "/api/kyc"),
        0x7D03 => (Method::POST, "/api/kyc/documents/b64"),
        // Crypto
        0x8E01 => (Method::GET, "/api/crypto/settings"),
        0x8E02 => (Method::GET, "/api/crypto/deposit-address"),
        0x8E03 => (Method::GET, "/api/crypto/quote/deposit"),
        0x8E04 => (Method::GET, "/api/crypto/quote/withdraw"),
        0x8E05 => (Method::POST, "/api/crypto/withdraw"),
        0x8E06 => (Method::GET, "/api/crypto/deposits"),
        0x8E07 => (Method::GET, "/api/crypto/withdrawals"),
        // Account
        0x9F01 => (Method::GET, "/api/account"),
        0x9F02 => (Method::POST, "/api/account/profile"),
        0x9F10 => (Method::GET, "/api/account/sessions"),
        0x9F11 => (Method::POST, "/api/account/sessions/{i}"),
        0x9F12 => (Method::POST, "/api/account/sessions/revoke-others"),
        0x9F20 => (Method::GET, "/api/account/events"),
        0x9F30 => (Method::GET, "/api/account/data-requests"),
        0x9F31 => (Method::POST, "/api/account/data-requests"),
        // Notifications + session
        0xA011 => (Method::GET, "/api/notifications"),
        0xA012 => (Method::POST, "/api/notifications/read-all"),
        0xA013 => (Method::POST, "/api/notifications/{i}/read"),
        0xA020 => (Method::GET, "/api/session/heartbeat"),
        // Passkey / auth (wire-visible only as opcodes)
        0xB101 => (Method::POST, "/api/register/begin"),
        0xB102 => (Method::POST, "/api/register/finish"),
        0xB111 => (Method::POST, "/api/login/begin"),
        0xB112 => (Method::POST, "/api/login/finish"),
        0xB121 => (Method::POST, "/api/step-up/begin"),
        0xB122 => (Method::POST, "/api/step-up/finish"),
        0xB130 => (Method::POST, "/api/logout"),
        0xB140 => (Method::GET, "/api/rp"),
        0xB150 => (Method::GET, "/api/me"),
        // Documents
        0xC101 => (Method::GET, "/api/documents/statement"),
        0xC102 => (Method::GET, "/api/documents/receipt/{i}"),
        0xC110 => (Method::GET, "/api/documents/dsm"),
        0xC111 => (Method::POST, "/api/documents/dsm/verify"),
        _ => return None,
    };

    let mut path = path_tpl.to_string();
    if path.contains("{a}") {
        let a = args.get("a")?;
        path = path.replace("{a}", a);
    }
    if path.contains("{i}") {
        let i = args.get("i")?;
        path = path.replace("{i}", i);
    }

    let mut query = Vec::new();
    for (k, v) in args {
        if k == "a" || k == "i" {
            continue;
        }
        let full = match k.as_str() {
            "h" => "handle",
            "f" => "from",
            "t" => "to",
            "l" => "limit",
            "u" => "usdt_minor",
            "r" => "rub_minor",
            "c" => "account_id",
            other => other,
        };
        query.push((full.to_string(), v.clone()));
    }

    Some(OpTarget {
        method,
        path,
        query,
    })
}
