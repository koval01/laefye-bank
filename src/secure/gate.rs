//! Reject plaintext bank/account JSON — customers must use `/api/secure/rpc`.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

pub async fn require_secure_channel(req: Request, next: Next) -> Response {
    let path = req.uri().path();
    // PDF statements/receipts and KYC multipart uploads stay on clear HTTP (cookie + TLS).
    if path.starts_with("/api/kyc/documents") || path.starts_with("/api/documents/") {
        return next.run(req).await;
    }
    (
        StatusCode::GONE,
        Json(serde_json::json!({
            "code": "secure_channel_required",
            "error": "use encrypted channel",
        })),
    )
        .into_response()
}
