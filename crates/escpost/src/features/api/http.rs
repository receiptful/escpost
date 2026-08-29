use axum::extract::{DefaultBodyLimit, State};
use axum::routing::get;
use axum::{Json, Router, middleware};
use serde::Serialize;
use tokio::net::TcpListener;

use super::{ApiState, CAPABILITIES, origin};

#[derive(Serialize)]
struct InfoResponse {
    version: &'static str,
    platform: &'static str,
    capabilities: &'static [&'static str],
}

pub(super) fn router(state: ApiState) -> Router {
    Router::new()
        .route("/info", get(info))
        .merge(super::printers::router())
        .merge(super::print::router())
        // A receipt is kilobytes; a job carrying a full-width raster image is
        // still well under this. Stated rather than inherited so the limit is
        // a decision someone can find.
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        // D2 applies to every route, including ones added later, because the
        // layer wraps the router rather than each handler.
        .layer(middleware::from_fn_with_state(state.clone(), origin::guard))
        .with_state(state)
}

pub(super) async fn serve(listener: TcpListener, state: ApiState) -> std::io::Result<()> {
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn info(State(_state): State<ApiState>) -> Json<InfoResponse> {
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
