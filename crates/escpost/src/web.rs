use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::routing::{any, get};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, watch};

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

/// How long the server waits for its open requests after Ctrl+C. Event streams
/// end as soon as they see the shutdown, so this limits only a request that
/// does not watch it, such as a discovery sweep that still runs. Without a
/// limit one such request stops Ctrl+C from working at all.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct WebState {
    jobs: JobStore,
    status_metadata: status::ServerStatusMetadata,
    /// Becomes true when the server starts to stop. Event streams watch it and
    /// end themselves, because the server waits for every open request.
    pub(crate) shutdown: watch::Receiver<bool>,
    pub(crate) printer_monitor: crate::features::printers::monitor::PrinterMonitor,
    pub(crate) printer_config: Option<PathBuf>,
    pub(crate) extension_id: Option<String>,
    pub(crate) job_sequence: Arc<AtomicU64>,
    pub(crate) printer_locks: PrinterLocks,
}

pub(crate) type PrinterLocks = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;

#[derive(Clone, Debug, Default)]
pub(crate) struct WebConfiguration {
    pub(crate) printer_config: Option<PathBuf>,
    pub(crate) extension_id: Option<String>,
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
    configuration: WebConfiguration,
) -> std::io::Result<()> {
    let status_metadata = status::ServerStatusMetadata::resolve(virtual_printer_address);
    let (shutdown, watch_shutdown) = watch::channel(false);
    let watch_grace = watch_shutdown.clone();
    let mut router = Router::new()
        .merge(crate::features::printers::http::router())
        .merge(crate::features::profiles::http::router())
        .merge(status::route())
        .merge(jobs::router())
        .merge(crate::features::api::router())
        .route("/health", get(health))
        .route("/api", any(error::not_found))
        .route("/api/{*path}", any(error::not_found));
    if web_app {
        router = router
            .route("/", get(frontend::index))
            .route("/assets/{*path}", get(frontend::asset))
            .route("/{*path}", get(frontend::index));
    }
    let state = WebState {
        jobs,
        status_metadata,
        shutdown: watch_shutdown,
        printer_monitor: crate::features::printers::monitor::PrinterMonitor::new(
            configuration.printer_config.clone(),
        ),
        printer_config: configuration.printer_config,
        extension_id: configuration.extension_id,
        job_sequence: Arc::new(AtomicU64::new(0)),
        printer_locks: Arc::new(Mutex::new(HashMap::new())),
    };
    // The print-only origin middleware reads the same state as its handler.
    // Axum keeps router state outside request extensions, so expose one clone
    // to that middleware before installing the handler state.
    let router = router
        .layer(axum::Extension(state.clone()))
        .with_state(state);
    let server = axum::serve(listener, router).with_graceful_shutdown(shutdown_signal(shutdown));
    // Stop when the last request is done, or when the grace period after the
    // signal is over, whichever comes first.
    tokio::select! {
        result = server => result,
        () = grace_over(watch_grace) => Ok(()),
    }
}

/// Liveness check for containers and automated tests. Returns 200 while the
/// server is accepting requests, independent of whether any job was captured.
async fn health() -> &'static str {
    "ok"
}

/// Wait for Ctrl+C, then tell the open event streams to end. The server keeps
/// running until every open request is done, and a status stream stays open
/// until something tells it to stop.
async fn shutdown_signal(shutdown: watch::Sender<bool>) {
    // Failure to install a signal handler should stop the server rather than
    // leave a foreground developer command that cannot shut down cleanly.
    let _ = tokio::signal::ctrl_c().await;
    let _ = shutdown.send(true);
}

/// Return when the grace period after the shutdown signal is over. The signal
/// task drops the sender as soon as it has sent, so a closed channel also means
/// the server is stopping.
async fn grace_over(mut shutdown: watch::Receiver<bool>) {
    while !*shutdown.borrow_and_update() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
    tokio::time::sleep(SHUTDOWN_GRACE).await;
}

#[cfg(test)]
mod tests {
    use std::pin::pin;

    use super::*;

    /// The grace period is the last defence: it stops the server even when a
    /// request never ends. It must start at the signal and not before, so a
    /// server that nobody interrupts keeps running.
    #[tokio::test(start_paused = true)]
    async fn the_grace_period_starts_at_the_shutdown_signal() {
        let (shutdown, receiver) = watch::channel(false);
        let mut grace = pin!(grace_over(receiver));

        assert!(
            tokio::time::timeout(SHUTDOWN_GRACE * 10, &mut grace)
                .await
                .is_err(),
            "the grace period should not run before the signal"
        );

        shutdown
            .send(true)
            .expect("the grace period should still watch the signal");

        assert!(
            tokio::time::timeout(SHUTDOWN_GRACE * 2, &mut grace)
                .await
                .is_ok(),
            "the grace period should end after the signal"
        );
    }
}
