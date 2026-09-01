//! Shared API failure envelope for print endpoints.

use axum::Json;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug)]
pub(super) struct ApiFailure {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

impl ApiFailure {
    pub(super) fn unsupported_media_type() -> Self {
        Self::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "UNSUPPORTED_MEDIA_TYPE",
            "Print requests must use application/octet-stream.",
        )
    }

    pub(super) fn printer_required() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "PRINTER_REQUIRED",
            "Name a printer with ?printer=.",
        )
    }

    pub(super) fn origin_not_granted() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "ORIGIN_NOT_GRANTED",
            "This origin is not allowed to print.",
        )
    }

    pub(super) fn invalid_request() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "The print request is invalid.",
        )
    }

    pub(super) fn payload_too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "PAYLOAD_TOO_LARGE",
            "Print requests must not exceed 8 MiB.",
        )
    }

    pub(super) fn from_resolve_failure(error: crate::application::ApplicationError) -> Self {
        match error {
            crate::application::ApplicationError::UnknownConfiguredPrinter(_) => Self::new(
                StatusCode::NOT_FOUND,
                "PRINTER_NOT_FOUND",
                "The named printer is not configured.",
            ),
            error => Self::from_print_failure(error),
        }
    }

    pub(super) fn from_print_failure(_error: crate::application::ApplicationError) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PRINT_FAILED",
            "The print job could not be sent.",
        )
    }

    fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
        }
    }
}

#[cfg(test)]
impl ApiFailure {
    pub(super) fn status(&self) -> StatusCode {
        self.status
    }

    pub(super) fn code(&self) -> &'static str {
        self.code
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
