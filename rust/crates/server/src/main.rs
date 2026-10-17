//! OmniRoute Rust server — Phase 0 skeleton.
//!
//! Minimal axum server: `GET /healthz` + graceful shutdown, honoring the `PORT`
//! env var (default 20128) to match the TS server's port contract. The full API
//! surface is implemented phase by phase; see
//! `_tasks/superpowers/plans/2026-10-17-rust-backend-migration.md`.
//!
//! The `/healthz` payload here is a stub — byte parity with the TS server is
//! established by contract fixtures (`tests/contract/rust-parity/`) in Phase 1.

use axum::{Json, Router, routing::get};
use serde_json::json;
use std::net::SocketAddr;
use tracing::info;

const DEFAULT_PORT: u16 = 20128;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("APP_LOG_LEVEL")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let port = std::env::var("PORT")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let app = Router::new().route("/healthz", get(healthz));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind listener");

    info!("server listening on http://{addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server exited with error");
}

async fn healthz() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install SIGINT handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    info!("shutdown signal received — draining");
}

#[test]
fn server_crate_scaffolded() {
    assert_eq!(DEFAULT_PORT, 20128);
}
