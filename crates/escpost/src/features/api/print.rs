use super::error::ApiFailure;

#[derive(Debug)]
struct PrintRequest {
    printer: String,
    bytes: Vec<u8>,
}

fn decode_payload(
    content_type: &str,
    printer: Option<&str>,
    body: &[u8],
) -> Result<PrintRequest, ApiFailure> {
    let media_type = content_type
        .split_once(';')
        .map_or(content_type, |(media_type, _)| media_type)
        .trim();
    if media_type != "application/octet-stream" {
        return Err(ApiFailure::unsupported_media_type());
    }

    let Some(printer) = printer.filter(|printer| !printer.trim().is_empty()) else {
        return Err(ApiFailure::printer_required());
    };

    Ok(PrintRequest {
        printer: printer.to_owned(),
        bytes: body.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    use super::decode_payload;
    use crate::application::ApplicationError;
    use crate::features::api::error::ApiFailure;

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
        let error = ApiFailure::from_resolve_failure(ApplicationError::UnknownConfiguredPrinter(
            "counter".to_owned(),
        ));
        assert_eq!(error.status(), StatusCode::NOT_FOUND);
        assert_eq!(error.code(), "UNKNOWN_PRINTER");
    }

    #[test]
    fn a_print_failure_maps_to_internal_server_error() {
        let error = ApiFailure::from_print_failure(ApplicationError::BlankPrinterName);
        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error.code(), "PRINT_FAILED");
    }

    #[tokio::test]
    async fn api_failures_use_the_no_store_envelope() {
        let response = ApiFailure::printer_required().into_response();
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
}
