use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::header;
use axum::routing::get;
use serde::Serialize;

use super::WebState;

#[derive(Serialize)]
pub(super) struct StatusResponse {
    virtual_printer: Option<VirtualPrinterStatus>,
    jobs_processed: u64,
    config_path: String,
}

#[derive(Serialize)]
pub(super) struct VirtualPrinterStatus {
    state: &'static str,
    address: String,
}

pub(super) async fn status(
    State(state): State<WebState>,
) -> (
    [(axum::http::HeaderName, &'static str); 1],
    Json<StatusResponse>,
) {
    let runtime = state.jobs.runtime_status().await;
    let virtual_printer = state
        .virtual_printer_address
        .map(|address| VirtualPrinterStatus {
            state: if runtime.receiving {
                "receiving"
            } else {
                "ready"
            },
            address: address.to_string(),
        });

    // Every printer command prints the file it writes to; the browser is
    // about to start writing printers to that same file, so it needs to see
    // which one. Status must never fail because configuration could not be
    // resolved, so a resolution error degrades to an empty string instead of
    // taking down the endpoint that reports server health.
    let config_path = crate::configuration::resolved_path(None)
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(StatusResponse {
            virtual_printer,
            jobs_processed: runtime.jobs_processed,
            config_path,
        }),
    )
}

pub(super) fn route() -> Router<WebState> {
    Router::new().route(
        "/api/status",
        get(status).fallback(super::error::method_not_allowed),
    )
}
