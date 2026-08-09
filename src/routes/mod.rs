//! HTTP routers — role-split: public customer edge vs corp admin.
//!
//! Public edge exposes only `POST /api` (+ healthz / debug tunnel RP).
//! All business + passkey handlers are reached via encrypted oneshot dispatch.

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
use tower_http::trace::TraceLayer;

use crate::config::AppRole;
use crate::host_gate::block_localhost_host;
use crate::AppState;

pub fn router(state: AppState) -> Router {
    let role = state.app_role;
    let mut app = Router::new().route("/healthz", get(|| async { "ok" }));

    match role {
        AppRole::Public => {
            #[cfg(feature = "customer-api")]
            {
                tracing::info!("router role=public — single POST /api surface");
                app = app
                    .merge(crate::secure::router(state.clone()))
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
                app = app
                    .merge(passkey_rs::axum::router(state.passkey.clone()))
                    .merge(admin::router(state.clone()));
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
