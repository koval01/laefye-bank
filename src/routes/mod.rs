//! HTTP routers — role-split: public customer edge vs corp admin.

#[cfg(feature = "admin-api")]
pub mod admin;
#[cfg(feature = "customer-api")]
pub mod account;
#[cfg(feature = "customer-api")]
pub mod bank;
#[cfg(feature = "customer-api")]
pub mod debug;
#[cfg(feature = "customer-api")]
pub mod notify;

use axum::middleware;
use axum::routing::get;
use axum::Router;
use passkey_rs::axum::router as passkey_router;
use tower_http::trace::TraceLayer;

use crate::config::AppRole;
use crate::host_gate::block_localhost_host;
use crate::AppState;

pub fn router(state: AppState) -> Router {
    let role = state.app_role;
    let mut app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(passkey_router(state.passkey.clone()));

    match role {
        AppRole::Public => {
            #[cfg(feature = "customer-api")]
            {
                tracing::info!("router role=public (customer edge — no admin API)");
                let gated = axum::Router::new()
                    .merge(bank::router(state.clone()))
                    .merge(account::router(state.clone()))
                    .merge(notify::router(state.clone()))
                    .layer(middleware::from_fn(crate::secure::require_secure_channel));
                app = app
                    .merge(crate::secure::router(state.clone()))
                    .merge(notify::heartbeat_router(state.clone()))
                    .merge(bank::dsm_public_router(state.clone()))
                    .merge(gated)
                    .merge(debug::router(state.clone()));
            }
            #[cfg(not(feature = "customer-api"))]
            {
                tracing::error!(
                    "APP_ROLE=public but this binary was built without feature customer-api"
                );
            }
        }
        AppRole::Admin => {
            #[cfg(feature = "admin-api")]
            {
                tracing::info!("router role=admin (corp network — admin API only)");
                app = app.merge(admin::router(state.clone()));
            }
            #[cfg(not(feature = "admin-api"))]
            {
                tracing::error!(
                    "APP_ROLE=admin but this binary was built without feature admin-api"
                );
            }
        }
    }

    let app = app.layer(TraceLayer::new_for_http());
    if state.block_localhost {
        app.layer(middleware::from_fn(block_localhost_host))
    } else {
        app
    }
}
