use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::extract::State;
use axum::http::{HeaderValue, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_core::Stream;
use serde::Serialize;
use tokio::sync::mpsc;

use super::{JobRuntimeStatus, WebState};

#[derive(Clone)]
pub(super) struct ServerStatusMetadata {
    virtual_printer_address: Option<SocketAddr>,
    config_path: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct ServerStatusSnapshot {
    virtual_printer: Option<VirtualPrinterStatus>,
    jobs_processed: u64,
    config_path: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct VirtualPrinterStatus {
    state: &'static str,
    address: String,
}

impl ServerStatusMetadata {
    pub(super) fn resolve(virtual_printer_address: Option<SocketAddr>) -> Self {
        // Every printer command prints the file it writes to; the browser is
        // about to start writing printers to that same file, so it needs to see
        // which one. Status must never fail because configuration could not be
        // resolved, so a resolution error degrades to an empty string instead
        // of taking down the endpoint that reports server health.
        let config_path = crate::configuration::resolved_path(None)
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        Self {
            virtual_printer_address,
            config_path,
        }
    }
}

impl ServerStatusSnapshot {
    pub(super) fn new(metadata: &ServerStatusMetadata, runtime: &JobRuntimeStatus) -> Self {
        let virtual_printer =
            metadata
                .virtual_printer_address
                .map(|address| VirtualPrinterStatus {
                    state: if runtime.receiving {
                        "receiving"
                    } else {
                        "ready"
                    },
                    address: address.to_string(),
                });
        Self {
            virtual_printer,
            jobs_processed: runtime.jobs_processed,
            config_path: metadata.config_path.clone(),
        }
    }
}

struct StatusStream {
    receiver: mpsc::Receiver<Result<Event, Infallible>>,
}

impl Stream for StatusStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(context)
    }
}

pub(super) async fn status(
    State(state): State<WebState>,
) -> (
    [(axum::http::HeaderName, &'static str); 1],
    Json<ServerStatusSnapshot>,
) {
    let runtime = state.jobs.runtime_status();
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(ServerStatusSnapshot::new(&state.status_metadata, &runtime)),
    )
}

pub(super) async fn events(State(state): State<WebState>) -> Response {
    let mut receiver = state.jobs.subscribe_runtime_status();
    let metadata = state.status_metadata.clone();
    let (sender, event_receiver) = mpsc::channel(1);
    tokio::spawn(async move {
        loop {
            let runtime = receiver.borrow_and_update().clone();
            if sender
                .send(Ok(status_event(&metadata, &runtime)))
                .await
                .is_err()
            {
                break;
            }
            tokio::select! {
                _ = sender.closed() => break,
                changed = receiver.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut response = Sse::new(StatusStream {
        receiver: event_receiver,
    })
    .keep_alive(KeepAlive::default())
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn status_event(metadata: &ServerStatusMetadata, runtime: &JobRuntimeStatus) -> Event {
    let snapshot = ServerStatusSnapshot::new(metadata, runtime);
    Event::default().event("status").data(
        serde_json::to_string(&snapshot)
            .expect("server status snapshots contain only serializable fields"),
    )
}

pub(super) fn route() -> Router<WebState> {
    Router::new()
        .route(
            "/api/status",
            get(status).fallback(super::error::method_not_allowed),
        )
        .route(
            "/api/status/events",
            get(events).fallback(super::error::method_not_allowed),
        )
}
