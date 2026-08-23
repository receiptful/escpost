use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;

use super::CAPABILITIES;

#[derive(Serialize)]
struct InfoResponse {
    version: &'static str,
    platform: &'static str,
    capabilities: &'static [&'static str],
}

pub(super) fn router() -> Router {
    Router::new().route("/info", get(info))
}

pub(super) async fn serve(listener: TcpListener) -> std::io::Result<()> {
    axum::serve(listener, router())
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn info() -> Json<InfoResponse> {
    Json(InfoResponse {
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        capabilities: CAPABILITIES,
    })
}

async fn shutdown_signal() {
    // Matches web.rs: a failed signal handler should stop the server rather
    // than leave a foreground command that cannot be shut down cleanly.
    let _ = tokio::signal::ctrl_c().await;
}
