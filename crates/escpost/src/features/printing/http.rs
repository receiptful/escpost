use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, header};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router, middleware};
use serde::{Deserialize, Serialize};
use tokio::sync::OwnedMutexGuard;

use crate::application::{self, ApplicationError};
use crate::features::printing::{self, ResolveRequest};
use crate::web::{WebState, error::ApiError, origin};

#[derive(Debug)]
struct PrintRequest {
    printer: String,
    bytes: Vec<u8>,
}

fn decode_payload(
    content_type: &str,
    printer: Option<&str>,
    body: &[u8],
) -> Result<PrintRequest, ApiError> {
    let media_type = content_type
        .split_once(';')
        .map_or(content_type, |(media_type, _)| media_type)
        .trim();
    if media_type != "application/octet-stream" {
        return Err(ApiError::unsupported_media_type());
    }

    let Some(printer) = printer.filter(|printer| !printer.trim().is_empty()) else {
        return Err(ApiError::printer_required());
    };

    Ok(PrintRequest {
        printer: printer.to_owned(),
        bytes: body.to_vec(),
    })
}

pub(crate) fn router() -> Router<WebState> {
    Router::new()
        .route("/api/print", post(print_job))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .route_layer(middleware::from_fn(origin::guard))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrintQuery {
    printer: Option<String>,
}

#[derive(Serialize)]
struct PrintResponse {
    job_id: String,
}

async fn printer_lock(state: &WebState, printer: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = state.printer_locks.lock().await;
    Arc::clone(
        locks
            .entry(printer.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

async fn spawn_guarded_print<Output, Operation>(
    guard: OwnedMutexGuard<()>,
    operation: Operation,
) -> application::Result<Output>
where
    Output: Send + 'static,
    Operation: Future<Output = application::Result<Output>> + Send + 'static,
{
    tokio::spawn(async move {
        let _printing = guard;
        operation.await
    })
    .await
    .map_err(ApplicationError::PrintTaskFailed)?
}

async fn print_job(
    State(state): State<WebState>,
    query: Result<Query<PrintQuery>, QueryRejection>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(query) = query.map_err(|_| ApiError::invalid_request())?;
    let body = body.map_err(|rejection| {
        let status = rejection.into_response().status();
        if status == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::payload_too_large()
        } else {
            ApiError::invalid_request()
        }
    })?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let request = decode_payload(content_type, query.printer.as_deref(), &body)?;
    let printer = printing::resolve_target(ResolveRequest {
        printer_name: request.printer.clone(),
        config: state.printer_config.clone(),
    })
    .map_err(ApiError::from_resolve_failure)?;

    let lock = printer_lock(&state, &request.printer).await;
    let guard = lock.lock_owned().await;
    spawn_guarded_print(
        guard,
        printing::print(printing::Request {
            bytes: request.bytes,
            printer,
        }),
    )
    .await
    .map_err(ApiError::from_print_failure)?;

    let sequence = state.job_sequence.fetch_add(1, Ordering::Relaxed);
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(PrintResponse {
            job_id: format!("job-{sequence}"),
        }),
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use tokio::sync::{Mutex, oneshot};
    use tokio::time::timeout;

    use super::{decode_payload, spawn_guarded_print};
    use crate::application::ApplicationError;
    use crate::web::error::ApiError;

    #[test]
    fn octet_stream_preserves_every_byte() {
        let request = decode_payload(
            "application/octet-stream",
            Some("counter"),
            &[0x1b, 0x40, 0x00, 0xff, 0x0a],
        )
        .expect("raw bytes should be accepted");
        assert_eq!(request.printer, "counter");
        assert_eq!(request.bytes, [0x1b, 0x40, 0x00, 0xff, 0x0a]);
    }

    #[test]
    fn octet_stream_trims_whitespace_before_parameters() {
        let request = decode_payload(
            "application/octet-stream ; charset=binary",
            Some("counter"),
            &[0x1b, 0x40],
        )
        .expect("whitespace before parameters should not change the media type");
        assert_eq!(request.printer, "counter");
        assert_eq!(request.bytes, [0x1b, 0x40]);
    }

    #[test]
    fn json_is_not_a_supported_print_media_type() {
        let error = decode_payload("application/json", Some("counter"), br#"{}"#)
            .expect_err("JSON must not become a second print contract");
        assert_eq!(error.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[test]
    fn a_printer_query_is_required() {
        let error = decode_payload("application/octet-stream", None, b"job")
            .expect_err("the body contains content, not routing metadata");
        assert_eq!(error.code(), "PRINTER_REQUIRED");
    }

    #[test]
    fn an_unknown_printer_maps_to_not_found() {
        let error = ApiError::from_resolve_failure(ApplicationError::UnknownConfiguredPrinter(
            "counter".to_owned(),
        ));
        assert_eq!(error.status(), StatusCode::NOT_FOUND);
        assert_eq!(error.code(), "PRINTER_NOT_FOUND");
    }

    #[test]
    fn a_print_failure_maps_to_internal_server_error() {
        let error = ApiError::from_print_failure(ApplicationError::BlankPrinterName);
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.code(), "PRINT_FAILED");
    }

    #[tokio::test]
    async fn api_failures_use_the_no_store_envelope() {
        let response = ApiError::printer_required().into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the JSON envelope should be readable");
        assert_eq!(
            body.as_ref(),
            br#"{"error":{"code":"PRINTER_REQUIRED","message":"Name a printer with ?printer=."}}"#,
        );
    }

    #[tokio::test]
    async fn cancelling_http_caller_keeps_printer_locked_until_physical_transfer_finishes() {
        // Defect caught: cancelling the HTTP awaiter drops its guard while an
        // already-started physical transfer continues in a detached task.
        let printer_lock = Arc::new(Mutex::new(()));
        let first_guard = Arc::clone(&printer_lock).lock_owned().await;
        let (physical_started_tx, physical_started_rx) = oneshot::channel();
        let (release_physical_tx, release_physical_rx) = oneshot::channel();

        let caller = tokio::spawn(spawn_guarded_print(first_guard, async move {
            physical_started_tx
                .send(())
                .expect("the test should observe the physical transfer");
            release_physical_rx
                .await
                .expect("the test should release the physical transfer");
            Ok::<(), ApplicationError>(())
        }));
        physical_started_rx
            .await
            .expect("the physical transfer should start");

        caller.abort();
        assert!(
            caller
                .await
                .expect_err("the HTTP caller should be cancelled")
                .is_cancelled()
        );
        assert!(
            Arc::clone(&printer_lock).try_lock_owned().is_err(),
            "a second job must not acquire the printer during the physical transfer"
        );

        release_physical_tx
            .send(())
            .expect("the physical transfer should still be running");
        timeout(Duration::from_secs(1), printer_lock.lock_owned())
            .await
            .expect("the printer should unlock after the physical transfer completes");
    }
}
