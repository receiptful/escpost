use axum::Json;
use axum::extract::State;
use axum::http::header;
use serde::Serialize;

use super::WebState;

#[derive(Serialize)]
pub(super) struct StatusResponse {
    virtual_printer: Option<VirtualPrinterStatus>,
    jobs_processed: u64,
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

    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(StatusResponse {
            virtual_printer,
            jobs_processed: runtime.jobs_processed,
        }),
    )
}
