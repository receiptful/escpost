use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::routing::{any, get};
use tokio::net::TcpListener;

mod commands;
pub(crate) mod error;
mod frontend;
mod job_store;
mod jobs;
mod status;

pub(crate) use commands::{CommandResponse, command_responses};
pub(crate) use job_store::JobStore;
use job_store::{JobRuntimeStatus, RenderedJob};

const FIRST_AUTOMATIC_PORT: u16 = 9000;
const LAST_AUTOMATIC_PORT: u16 = 9099;

#[derive(Clone)]
pub(crate) struct WebState {
    jobs: JobStore,
    status_metadata: status::ServerStatusMetadata,
    pub(crate) printer_monitor: crate::features::printers::monitor::PrinterMonitor,
}

/// Current wall-clock time in Unix epoch milliseconds, for job completion.
fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) async fn bind(
    requested: Option<SocketAddr>,
) -> Result<TcpListener, crate::net::BindFailure> {
    crate::net::bind_loopback(requested, FIRST_AUTOMATIC_PORT..=LAST_AUTOMATIC_PORT).await
}

pub(crate) async fn serve(
    listener: TcpListener,
    jobs: JobStore,
    virtual_printer_address: Option<SocketAddr>,
    web_app: bool,
) -> std::io::Result<()> {
    let status_metadata = status::ServerStatusMetadata::resolve(virtual_printer_address);
    let mut router = Router::new()
        .merge(crate::features::printers::http::router())
        .merge(crate::features::profiles::http::router())
        .merge(status::route())
        .merge(jobs::router())
        .route("/health", get(health))
        .route("/api", any(error::not_found))
        .route("/api/{*path}", any(error::not_found));
    if web_app {
        router = router
            .route("/", get(frontend::index))
            .route("/assets/{*path}", get(frontend::asset))
            .route("/{*path}", get(frontend::index));
    }
    let router = router.with_state(WebState {
        jobs,
        status_metadata,
        printer_monitor: crate::features::printers::monitor::PrinterMonitor::new(None),
    });
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
}

/// Liveness check for containers and automated tests. Returns 200 while the
/// server is accepting requests, independent of whether any job was captured.
async fn health() -> &'static str {
    "ok"
}

async fn shutdown_signal() {
    // Failure to install a signal handler should stop the server rather than
    // leave a foreground developer command that cannot shut down cleanly.
    let _ = tokio::signal::ctrl_c().await;
}
