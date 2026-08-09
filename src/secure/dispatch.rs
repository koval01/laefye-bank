//! Decrypt RpcRequest (opaque op) → oneshot internal JSON handlers → protobuf response.

use axum::body::Body;
use axum::http::{Method, Request};
use prost::Message;
use tower::ServiceExt;

use crate::pb;
use crate::routes::{account, bank};
use crate::AppState;

use super::ops;

pub async fn dispatch(
    state: AppState,
    cookie_header: Option<String>,
    origin: Option<String>,
    forward_headers: Vec<(String, String)>,
    rpc: pb::RpcRequest,
) -> pb::RpcResponse {
    let Some(target) = ops::resolve(rpc.op, &rpc.args) else {
        return pb::RpcResponse {
            status: 404,
            body: vec![],
            error: "not found".into(),
            code: "not_found".into(),
        };
    };

    let json_body = match decode_request_json(&target.method, &rpc.body) {
        Ok(v) => v,
        Err(e) => {
            return pb::RpcResponse {
                status: 400,
                body: vec![],
                error: e,
                code: "bad_request".into(),
            };
        }
    };

    let uri = build_uri(&target.path, &target.query);
    let mut builder = Request::builder().method(target.method.clone()).uri(uri);
    if let Some(c) = cookie_header {
        builder = builder.header(axum::http::header::COOKIE, c);
    }
    if let Some(o) = origin {
        builder = builder.header(axum::http::header::ORIGIN, o);
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
        builder = builder.header(axum::http::header::CONTENT_TYPE, "application/json");
        Body::from(json_body)
    };
    let req = match builder.body(body) {
        Ok(r) => r,
        Err(_) => {
            return pb::RpcResponse {
                status: 400,
                body: vec![],
                error: "bad request".into(),
                code: "bad_request".into(),
            };
        }
    };

    let router = bank::router(state.clone())
        .merge(account::router(state.clone()))
        .merge(crate::routes::notify::router(state));
    let res = match router.oneshot(req).await {
        Ok(r) => r,
        Err(_) => {
            return pb::RpcResponse {
                status: 500,
                body: vec![],
                error: "internal error".into(),
                code: "internal".into(),
            };
        }
    };

    let status = res.status().as_u16() as u32;
    let bytes = match axum::body::to_bytes(res.into_body(), 2 * 1024 * 1024).await {
        Ok(b) => b.to_vec(),
        Err(_) => {
            return pb::RpcResponse {
                status: 500,
                body: vec![],
                error: "internal error".into(),
                code: "internal".into(),
            };
        }
    };

    if status >= 400 {
        let (error, code) = parse_error_json(&bytes);
        return pb::RpcResponse {
            status,
            body: vec![],
            error,
            code,
        };
    }

    match encode_response_protobuf(&bytes) {
        Ok(body) => pb::RpcResponse {
            status,
            body,
            error: String::new(),
            code: String::new(),
        },
        Err(e) => pb::RpcResponse {
            status: 500,
            body: vec![],
            error: e,
            code: "internal".into(),
        },
    }
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