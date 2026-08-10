//! Decrypt RpcRequest (opaque op) → oneshot internal handlers → protobuf response.
//! Forwards Set-Cookie from inner handlers onto the outer `POST /api` response.

use axum::body::Body;
use axum::http::{header, HeaderValue, Method, Request, StatusCode};
use base64::Engine;
use prost::Message;
use tower::ServiceExt;

use crate::pb;
#[cfg(feature = "customer-api")]
use crate::routes::{account, bank, notify};
use crate::AppState;

use super::ops;

pub struct DispatchResult {
    pub rpc: pb::RpcResponse,
    pub set_cookies: Vec<HeaderValue>,
}

pub async fn dispatch(
    state: AppState,
    cookie_header: Option<String>,
    origin: Option<String>,
    forward_headers: Vec<(String, String)>,
    rpc: pb::RpcRequest,
) -> DispatchResult {
    let Some(target) = ops::resolve(rpc.op, &rpc.args) else {
        return DispatchResult {
            rpc: pb::RpcResponse {
                status: 404,
                body: vec![],
                error: "not found".into(),
                code: "not_found".into(),
            },
            set_cookies: vec![],
        };
    };

    let json_body = match decode_request_json(&target.method, &rpc.body) {
        Ok(v) => v,
        Err(e) => {
            return DispatchResult {
                rpc: pb::RpcResponse {
                    status: 400,
                    body: vec![],
                    error: e,
                    code: "bad_request".into(),
                },
                set_cookies: vec![],
            };
        }
    };

    let uri = build_uri(&target.path, &target.query);
    let mut builder = Request::builder().method(target.method.clone()).uri(uri);
    if let Some(c) = cookie_header {
        builder = builder.header(header::COOKIE, c);
    }
    if let Some(o) = origin {
        builder = builder.header(header::ORIGIN, o);
    }
    for (k, v) in &forward_headers {
        if let (Ok(name), Ok(value)) = (
            axum::http::HeaderName::try_from(k.as_str()),
            axum::http::HeaderValue::try_from(v.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }
    let body = if json_body.is_empty() {
        Body::empty()
    } else {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(json_body)
    };
    let req = match builder.body(body) {
        Ok(r) => r,
        Err(_) => {
            return DispatchResult {
                rpc: pb::RpcResponse {
                    status: 400,
                    body: vec![],
                    error: "bad request".into(),
                    code: "bad_request".into(),
                },
                set_cookies: vec![],
            };
        }
    };

    // Internal-only routers (not mounted on the public edge).
    #[cfg(feature = "customer-api")]
    let router = bank::router(state.clone())
        .merge(bank::dsm_public_router(state.clone()))
        .merge(account::router(state.clone()))
        .merge(notify::router(state.clone()))
        .merge(passkey_rs::axum::router(state.passkey.clone()));

    #[cfg(not(feature = "customer-api"))]
    let router = axum::Router::new();

    let res = match router.oneshot(req).await {
        Ok(r) => r,
        Err(_) => {
            return DispatchResult {
                rpc: pb::RpcResponse {
                    status: 500,
                    body: vec![],
                    error: "internal error".into(),
                    code: "internal".into(),
                },
                set_cookies: vec![],
            };
        }
    };

    let set_cookies: Vec<HeaderValue> = res
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .cloned()
        .collect();

    let status = res.status().as_u16() as u32;
    let content_type = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let filename = res
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_filename)
        .unwrap_or_else(|| "document.pdf".into());

    let bytes = match axum::body::to_bytes(res.into_body(), 12 * 1024 * 1024).await {
        Ok(b) => b.to_vec(),
        Err(_) => {
            return DispatchResult {
                rpc: pb::RpcResponse {
                    status: 500,
                    body: vec![],
                    error: "internal error".into(),
                    code: "internal".into(),
                },
                set_cookies,
            };
        }
    };

    if status >= 400 {
        let (error, code) = parse_error_json(&bytes);
        return DispatchResult {
            rpc: pb::RpcResponse {
                status,
                body: vec![],
                error,
                code,
            },
            set_cookies,
        };
    }

    let encoded = if content_type.contains("application/pdf") || content_type.contains("octet-stream")
    {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let json = serde_json::json!({ "pdf_b64": b64, "filename": filename }).to_string();
        encode_response_protobuf(json.as_bytes())
    } else if content_type.contains("application/json") || bytes.first() == Some(&b'{') || bytes.first() == Some(&b'[')
    {
        encode_response_protobuf(&bytes)
    } else if bytes.is_empty() {
        Ok(pb::JsonCompat {
            json: "null".into(),
        }
        .encode_to_vec())
    } else {
        // Fallback: wrap raw text
        encode_response_protobuf(
            &serde_json::json!({ "raw": String::from_utf8_lossy(&bytes) })
                .to_string()
                .into_bytes(),
        )
    };

    match encoded {
        Ok(body) => DispatchResult {
            rpc: pb::RpcResponse {
                status,
                body,
                error: String::new(),
                code: String::new(),
            },
            set_cookies,
        },
        Err(e) => DispatchResult {
            rpc: pb::RpcResponse {
                status: StatusCode::INTERNAL_SERVER_ERROR.as_u16() as u32,
                body: vec![],
                error: e,
                code: "internal".into(),
            },
            set_cookies,
        },
    }
}

fn parse_filename(disp: &str) -> Option<String> {
    // attachment; filename="foo.pdf"
    let lower = disp.to_ascii_lowercase();
    let idx = lower.find("filename=")?;
    let mut rest = disp[idx + 9..].trim();
    if let Some(stripped) = rest.strip_prefix('"') {
        rest = stripped;
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_string());
        }
    }
    Some(
        rest.split(';')
            .next()
            .unwrap_or(rest)
            .trim()
            .to_string(),
    )
}

fn build_uri(path: &str, query: &[(String, String)]) -> String {
    if query.is_empty() {
        return path.to_string();
    }
    let mut parts: Vec<String> = query
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding_encode(k), urlencoding_encode(v)))
        .collect();
    parts.sort();
    format!("{path}?{}", parts.join("&"))
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn decode_request_json(method: &Method, body: &[u8]) -> Result<Vec<u8>, String> {
    if *method == Method::GET || body.is_empty() {
        return Ok(vec![]);
    }
    let j = pb::JsonCompat::decode(body).map_err(|_| "protobuf JsonCompat body required".to_string())?;
    Ok(j.json.into_bytes())
}

fn encode_response_protobuf(json_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let msg = pb::JsonCompat {
        json: String::from_utf8(json_bytes.to_vec()).map_err(|_| "non-utf8 json".to_string())?,
    };
    Ok(msg.encode_to_vec())
}

fn parse_error_json(bytes: &[u8]) -> (String, String) {
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) {
        let error = v
            .get("error")
            .and_then(|x| x.as_str())
            .unwrap_or("request failed")
            .to_string();
        let code = v
            .get("code")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        return (error, code);
    }
    ("request failed".into(), String::new())
}
