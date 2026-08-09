//! Single public surface: `POST /api` (protobuf ApiFrame).

use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Router, body::Bytes};
use prost::Message;
use rand::RngCore;

use crate::pb;
use crate::pb::api_frame::Payload;
use crate::AppState;

use super::channel::{channel_ttl_secs, ChallengeState};
use super::crypto::{
    derive_session_keys, decrypt, encrypt, generate_server_ephemeral, make_proof, parse_client_public,
    random_id, verify_proof,
};
use super::dispatch;
use super::pow;

const POW_DIFFICULTY: u32 = 16;
const MAX_SKEW_MS: i64 = 120_000;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api", post(api_entry))
        .with_state(state)
}

fn pb_frame(status: StatusCode, frame: &pb::ApiFrame) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        frame.encode_to_vec(),
    )
        .into_response()
}

fn frame_err(status: StatusCode, code: &str, error: &str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/x-protobuf")],
        pb::RpcResponse {
            status: status.as_u16() as u32,
            body: vec![],
            error: error.into(),
            code: code.into(),
        }
        .encode_to_vec(),
    )
        .into_response()
}

async fn api_entry(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(store) = state.secure.clone() else {
        return frame_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "secure channel unavailable",
        );
    };

    let frame = match pb::ApiFrame::decode(body.as_ref()) {
        Ok(f) => f,
        Err(_) => return frame_err(StatusCode::BAD_REQUEST, "bad_request", "invalid frame"),
    };

    match frame.payload {
        Some(Payload::ChallengeReq(_)) => challenge(&store).await,
        Some(Payload::OpenReq(req)) => open(&store, req).await,
        Some(Payload::Envelope(env)) => match handle_envelope(state, &store, headers, env).await {
            Ok((out, cookies)) => {
                let mut res = pb_frame(
                    StatusCode::OK,
                    &pb::ApiFrame {
                        payload: Some(Payload::Envelope(out)),
                    },
                );
                for c in cookies {
                    res.headers_mut().append(header::SET_COOKIE, c);
                }
                res
            }
            Err(r) => r,
        },
        Some(Payload::ChallengeRes(_)) | Some(Payload::OpenRes(_)) | None => {
            frame_err(StatusCode::BAD_REQUEST, "bad_request", "unexpected frame")
        }
    }
}

async fn mint_challenge(
    store: &crate::secure::ChannelStore,
) -> Result<pb::ChallengeResponse, Response> {
    let mut challenge = vec![0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut challenge);
    let id = random_id();
    if let Err(e) = store
        .put_challenge(
            &id,
            &ChallengeState {
                challenge: challenge.clone(),
                difficulty: POW_DIFFICULTY,
            },
        )
        .await
    {
        tracing::warn!(error = %e, "secure challenge store failed");
        return Err(frame_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "challenge failed",
        ));
    }
    Ok(pb::ChallengeResponse {
        challenge,
        difficulty: POW_DIFFICULTY,
        challenge_id: id,
    })
}

async fn challenge(store: &crate::secure::ChannelStore) -> Response {
    match mint_challenge(store).await {
        Ok(res) => pb_frame(
            StatusCode::OK,
            &pb::ApiFrame {
                payload: Some(Payload::ChallengeRes(res)),
            },
        ),
        Err(r) => r,
    }
}

async fn do_open(
    store: &crate::secure::ChannelStore,
    req: pb::OpenRequest,
) -> Result<pb::OpenResponse, Response> {
    let Some(chal) = (match store.take_challenge(&req.challenge_id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "take challenge failed");
            return Err(frame_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "open failed",
            ));
        }
    }) else {
        return Err(frame_err(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "challenge expired",
        ));
    };
    if !pow::verify(&chal.challenge, req.pow_nonce, chal.difficulty) {
        return Err(frame_err(
            StatusCode::BAD_REQUEST,
            "pow_failed",
            "proof of work failed",
        ));
    }
    let client_pk = match parse_client_public(&req.client_public_key) {
        Ok(pk) => pk,
        Err(e) => return Err(frame_err(StatusCode::BAD_REQUEST, "bad_request", &e)),
    };
    let (server_secret, server_pub) = generate_server_ephemeral();
    let channel_id = random_id();
    let keys = derive_session_keys(server_secret, &client_pk, &channel_id, &req.client_binding);
    if let Err(e) = store.put_channel(&channel_id, &keys).await {
        tracing::warn!(error = %e, "put channel failed");
        return Err(frame_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "open failed",
        ));
    }
    Ok(pb::OpenResponse {
        channel_id,
        server_public_key: server_pub,
        ttl_secs: channel_ttl_secs(),
    })
}

async fn open(store: &crate::secure::ChannelStore, req: pb::OpenRequest) -> Response {
    match do_open(store, req).await {
        Ok(res) => pb_frame(
            StatusCode::OK,
            &pb::ApiFrame {
                payload: Some(Payload::OpenRes(res)),
            },
        ),
        Err(r) => r,
    }
}

async fn handle_envelope(
    state: AppState,
    store: &crate::secure::ChannelStore,
    headers: HeaderMap,
    env: pb::SecureEnvelope,
) -> Result<(pb::SecureEnvelope, Vec<HeaderValue>), Response> {
    let mut ch = match store.load_channel(&env.channel_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Err(frame_err(
                StatusCode::UNAUTHORIZED,
                "channel_expired",
                "channel expired",
            ))
        }
        Err(e) => {
            tracing::warn!(error = %e, "load channel failed");
            return Err(frame_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "rpc failed",
            ));
        }
    };
    if env.counter <= ch.counter {
        return Err(frame_err(
            StatusCode::BAD_REQUEST,
            "replay",
            "counter replay",
        ));
    }
    let now = chrono::Utc::now().timestamp_millis();
    if (now - env.ts_ms).abs() > MAX_SKEW_MS {
        return Err(frame_err(
            StatusCode::BAD_REQUEST,
            "skew",
            "timestamp skew",
        ));
    }
    if !verify_proof(
        &ch.mac,
        &env.channel_id,
        env.counter,
        env.ts_ms,
        &env.nonce,
        &env.ciphertext,
        &env.proof,
    ) {
        return Err(frame_err(
            StatusCode::UNAUTHORIZED,
            "bad_proof",
            "invalid proof",
        ));
    }
    let plain = match decrypt(&ch.enc, &env.nonce, &env.ciphertext) {
        Ok(p) => p,
        Err(_) => {
            return Err(frame_err(
                StatusCode::BAD_REQUEST,
                "decrypt_failed",
                "decrypt failed",
            ))
        }
    };
    let rpc_req = match pb::RpcRequest::decode(plain.as_slice()) {
        Ok(r) => r,
        Err(_) => {
            return Err(frame_err(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "invalid rpc",
            ))
        }
    };

    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let mut forward = Vec::new();
    for key in [
        "cf-connecting-ip",
        "cf-ipcountry",
        "true-client-ip",
        "x-real-ip",
        "x-forwarded-for",
        "user-agent",
        "x-laefye-fingerprint",
    ] {
        if let Some(v) = headers.get(key).and_then(|v| v.to_str().ok()) {
            forward.push((key.to_string(), v.to_string()));
        }
    }

    let dispatched = dispatch::dispatch(state, cookie, origin, forward, rpc_req).await;
    let res_bytes = dispatched.rpc.encode_to_vec();
    let (nonce, ct) = match encrypt(&ch.enc, &res_bytes) {
        Ok(v) => v,
        Err(_) => {
            return Err(frame_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "encrypt failed",
            ))
        }
    };
    let out_counter = env.counter;
    let ts = chrono::Utc::now().timestamp_millis();
    let proof = make_proof(&ch.mac, &env.channel_id, out_counter, ts, &nonce, &ct);
    ch.counter = env.counter;
    if let Err(e) = store.save_channel(&env.channel_id, &ch).await {
        tracing::warn!(error = %e, "save channel failed");
    }
    Ok((
        pb::SecureEnvelope {
            channel_id: env.channel_id,
            counter: out_counter,
            ts_ms: ts,
            nonce,
            proof,
            ciphertext: ct,
        },
        dispatched.set_cookies,
    ))
}
