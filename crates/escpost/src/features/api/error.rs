use axum::Json;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// A failure on the extension-facing API.
///
/// Same envelope as `web::error::ApiError` — the extension parses
/// `{"error":{"code","message"}}` — but its own vocabulary. The codes here are
/// the spec's typed set, which the browser package re-exports; the viewer's
/// snake_case codes mean nothing to that client.
#[derive(Debug)]
pub(super) struct ApiFailure {
    status: StatusCode,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

impl ApiFailure {
    pub(super) fn origin_not_granted(origin: &str) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "ORIGIN_NOT_GRANTED",
            format!("The origin {origin} may not use this API."),
        )
    }

    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        (
            self.status,
            [(header::CACHE_CONTROL, "no-store")],
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}
