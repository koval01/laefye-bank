//! Laefye Bank API — HTTP behind Envoy / local HTTPS.

use std::net::SocketAddr;

use axum::response::Redirect;
use axum::routing::any;
use axum::Router;
use laefye_bank::config::AppConfig;
use laefye_bank::routes;
use laefye_bank::state_boot::build_app_state;
use laefye_bank::tls::load_or_generate_rustls_config;
use laefye_bank::tunnel::{host_and_origin, start_quick_tunnel, tunnel_requested};
use passkey_rs::axum::PasskeyState;
use passkey_rs::RelyingParty;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("aws-lc-rs crypto provider");

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "info,passkey_rs=debug,laefye_bank=debug,tower_http=info".into()
        }))
        .with(tracing_subscriber::fmt::layer().with_target(true).with_level(true))
        .init();

    let config = AppConfig::from_env();
    tracing::info!(
        http_only = config.http_only,
        block_localhost = config.block_localhost,
        app_role = ?config.app_role,
        "laefye-bank boot"
    );
    let state = build_app_state(&config).await.map_err(|e| e.to_string())?;
    let passkey = state.passkey.clone();
    let app = routes::router(state);

    if tunnel_requested() {
        run_with_tunnel(config, passkey, app).await
    } else if config.http_only {
        run_http_only(config, app).await
    } else {
        run_local_https(config, app).await
    }
}

async fn run_http_only(config: AppConfig, app: Router) -> Result<(), Box<dyn std::error::Error>> {
    let http_addr: SocketAddr = config.http_addr.parse()?;
    tracing::info!("HTTP-only → http://{http_addr} (Envoy / cloudflared)");
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn run_with_tunnel(
    config: AppConfig,
    state: PasskeyState,
    app: Router,
) -> Result<(), Box<dyn std::error::Error>> {
    let http_addr: SocketAddr = config.http_addr.parse()?;
    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    let bound = listener.local_addr()?;
    tracing::info!("local HTTP for tunnel → http://{bound}");

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("http serve");
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let tunnel = start_quick_tunnel(bound).map_err(|e| e.to_string())?;
    let (host, origin) = host_and_origin(&tunnel.public_url).map_err(|e| e.to_string())?;
    state.set_relying_party(RelyingParty::new(host, config.rp.name.clone(), origin.clone()));
    tracing::info!("public URL → {origin}");

    tokio::select! {
        res = server => res?,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutting down…");
        }
    }
    drop(tunnel);
    Ok(())
}

async fn run_local_https(
    config: AppConfig,
    app: Router,
) -> Result<(), Box<dyn std::error::Error>> {
    let tls = load_or_generate_rustls_config(&config.cert_dir)
        .await
        .map_err(|e| e.to_string())?;
    let https_addr: SocketAddr = config.https_addr.parse()?;
    let http_addr: SocketAddr = config.http_addr.parse()?;
    tracing::info!("HTTPS → https://localhost:{}", https_addr.port());

    let https_port = https_addr.port();
    let http_router = if config.serve_http_app {
        app.clone()
    } else {
        Router::new()
            .route(
                "/{*rest}",
                any(move |uri: axum::http::Uri| async move {
                    let path = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
                    Redirect::permanent(&format!("https://localhost:{https_port}{path}"))
                }),
            )
            .route(
                "/",
                any(move || async move {
                    Redirect::permanent(&format!("https://localhost:{https_port}/"))
                }),
            )
    };

    let http_server = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(http_addr)
            .await
            .expect("bind http");
        axum::serve(listener, http_router).await.expect("http serve");
    });

    let https_server = axum_server::bind_rustls(https_addr, tls).serve(app.into_make_service());
    tokio::select! {
        res = https_server => res?,
        res = http_server => res?,
    }
    Ok(())
}
