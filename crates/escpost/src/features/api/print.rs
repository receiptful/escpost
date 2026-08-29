use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, header};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use crate::features::printing::{self, ResolveRequest};

use super::ApiState;
use super::error::ApiFailure;

pub(super) fn router() -> Router<ApiState> {
    Router::new().route("/print", post(print_job))
}

#[derive(Deserialize)]
struct PrintQuery {
    printer: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonPrintBody {
    printer: String,
    /// Base64 of the exact ESC/POS bytes. The extension uses this variant; a
    /// local backend that already holds bytes should use octet-stream instead.
    data: String,
}

#[derive(Serialize)]
struct PrintResponse {
    job_id: String,
}

/// What a decoded request amounts to, whichever content type carried it.
#[derive(Debug)]
pub(super) struct PrintRequest {
    pub(super) printer: String,
    pub(super) bytes: Vec<u8>,
}

/// Turn either supported content type into one printer name and one byte
/// sequence, refusing anything ambiguous rather than guessing. Pure, so every
/// branch is a unit test instead of a live HTTP round trip.
pub(super) fn decode_payload(
    headers: &HeaderMap,
    query_printer: Option<&str>,
    body: &[u8],
) -> Result<PrintRequest, ApiFailure> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    // Compare only the media type: "application/json; charset=utf-8" is JSON.
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    let request = match media_type.as_str() {
        "application/json" => {
            let parsed: JsonPrintBody = serde_json::from_slice(body).map_err(|_| {
                ApiFailure::unsupported_format(
                    "The request body must be JSON of the form {\"printer\":…,\"data\":…}.",
                )
            })?;
            let bytes = STANDARD.decode(parsed.data.as_bytes()).map_err(|_| {
                ApiFailure::unsupported_format("The data field must be standard base64.")
            })?;
            PrintRequest {
                printer: parsed.printer,
                bytes,
            }
        }
        "application/octet-stream" => PrintRequest {
            printer: query_printer.unwrap_or_default().to_owned(),
            bytes: body.to_vec(),
        },
        _ => {
            return Err(ApiFailure::unsupported_format(
                "Send application/json or application/octet-stream.",
            ));
        }
    };

    if request.printer.trim().is_empty() {
        return Err(ApiFailure::printer_required());
    }
    Ok(request)
}

/// This printer's lock, created on first use.
///
/// Keyed by name rather than one global lock: two different printers must be
/// able to print at once, and the same printer must not — a USB device can only
/// be claimed once, and a RAW TCP printer is usually single-session.
pub(super) async fn printer_lock(state: &ApiState, printer: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = state.printer_locks.lock().await;
    Arc::clone(
        locks
            .entry(printer.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

async fn print_job(
    State(state): State<ApiState>,
    Query(query): Query<PrintQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<PrintResponse>, ApiFailure> {
    let request = decode_payload(&headers, query.printer.as_deref(), &body)?;

    let printer = printing::resolve_target(ResolveRequest {
        printer_name: request.printer.clone(),
        config: state.config.clone(),
    })
    .map_err(|_| ApiFailure::printer_not_found(&request.printer))?;

    // Held for the whole transfer, so two simultaneous prints to one printer
    // queue rather than collide.
    let lock = printer_lock(&state, &request.printer).await;
    let _printing = lock.lock().await;

    printing::print(printing::Request {
        bytes: request.bytes,
        printer,
    })
    .await
    .map_err(|error| ApiFailure::print_failed(&error.to_string()))?;

    let sequence = state.job_sequence.fetch_add(1, Ordering::Relaxed);
    Ok(Json(PrintResponse {
        job_id: format!("job-{sequence}"),
    }))
}

#[cfg(test)]
mod tests {
    use super::{decode_payload, printer_lock};
    use crate::features::api::ApiState;
    use axum::http::HeaderMap;
    use axum::http::header;
    use std::sync::Arc;
    use std::time::Duration;

    fn json_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
        headers
    }

    fn octet_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            "application/octet-stream".parse().unwrap(),
        );
        headers
    }

    #[test]
    fn a_json_body_yields_the_named_printer_and_the_decoded_bytes() {
        // "G0BIaQo=" is ESC @ H i LF: an initialize followed by two characters.
        let payload = decode_payload(
            &json_headers(),
            None,
            br#"{"printer":"counter","data":"G0BIaQo="}"#,
        )
        .expect("a well-formed JSON body should decode");

        assert_eq!(payload.printer, "counter");
        assert_eq!(payload.bytes, vec![0x1b, 0x40, b'H', b'i', b'\n']);
    }

    #[test]
    fn an_octet_stream_body_takes_the_printer_from_the_query_and_the_bytes_verbatim() {
        let payload = decode_payload(&octet_headers(), Some("counter"), &[0x1b, 0x40, 0xff])
            .expect("an octet-stream body should be taken verbatim");

        assert_eq!(payload.printer, "counter");
        // L2: exactly the caller's bytes, with no re-encoding on the way.
        assert_eq!(payload.bytes, vec![0x1b, 0x40, 0xff]);
    }

    #[test]
    fn a_content_type_with_a_charset_parameter_is_still_json() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            "application/json; charset=utf-8".parse().unwrap(),
        );

        let payload = decode_payload(&headers, None, br#"{"printer":"counter","data":""}"#)
            .expect("a parameterised content type should still be JSON");

        assert!(payload.bytes.is_empty());
    }

    #[test]
    fn invalid_base64_is_refused_rather_than_printed_as_rubbish() {
        // Guessing at malformed data would send an arbitrary byte sequence to a
        // physical printer.
        let error = decode_payload(
            &json_headers(),
            None,
            br#"{"printer":"counter","data":"not base64!!"}"#,
        )
        .expect_err("invalid base64 must be refused");
        assert_eq!(error.code(), "UNSUPPORTED_FORMAT");
    }

    #[test]
    fn a_json_body_that_is_not_the_expected_shape_is_refused() {
        let error = decode_payload(&json_headers(), None, br#"{"printer":"counter"}"#)
            .expect_err("a body with no data field must be refused");
        assert_eq!(error.code(), "UNSUPPORTED_FORMAT");
    }

    #[test]
    fn an_octet_stream_with_no_printer_names_the_missing_parameter() {
        let error = decode_payload(&octet_headers(), None, &[0x1b])
            .expect_err("octet-stream without ?printer= must be refused");
        assert_eq!(error.code(), "PRINTER_NOT_FOUND");
    }

    #[test]
    fn an_unknown_content_type_is_refused() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "text/plain".parse().unwrap());

        let error = decode_payload(&headers, Some("counter"), b"hello")
            .expect_err("an unsupported content type must be refused");
        assert_eq!(error.code(), "UNSUPPORTED_FORMAT");
    }

    #[test]
    fn a_request_naming_no_printer_at_all_is_refused() {
        let error = decode_payload(&json_headers(), None, br#"{"printer":"","data":""}"#)
            .expect_err("a blank printer name must be refused");
        assert_eq!(error.code(), "PRINTER_NOT_FOUND");
    }

    #[tokio::test]
    async fn one_printer_has_exactly_one_lock() {
        // The mutex has to be keyed, not global: two different printers must
        // print at the same time, and the same printer must not.
        let state = ApiState::default();

        let first = printer_lock(&state, "counter").await;
        let again = printer_lock(&state, "counter").await;
        let other = printer_lock(&state, "kitchen").await;

        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(&first, &other));
    }

    #[tokio::test]
    async fn a_second_print_to_the_same_printer_waits_for_the_first() {
        let state = ApiState::default();
        let held = printer_lock(&state, "counter").await;
        let guard = held.lock().await;

        let contended = printer_lock(&state, "counter").await;
        // try_lock rather than a sleep: this asserts contention rather than
        // hoping a timing window is wide enough.
        assert!(
            contended.try_lock().is_err(),
            "a second print to one printer must wait"
        );

        drop(guard);
        assert!(
            contended.try_lock().is_ok(),
            "the lock must be released when the print finishes"
        );
    }

    #[tokio::test]
    async fn two_different_printers_do_not_block_each_other() {
        let state = ApiState::default();
        let counter = printer_lock(&state, "counter").await;
        let _busy = counter.lock().await;

        let kitchen = printer_lock(&state, "kitchen").await;
        let free = tokio::time::timeout(Duration::from_millis(200), kitchen.lock()).await;

        assert!(free.is_ok(), "a different printer must not be blocked");
    }
}
