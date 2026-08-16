use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

pub(crate) struct ApiError {
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

impl ApiError {
    pub(crate) fn invalid_query() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "The query parameters are invalid.",
        )
    }

    pub(crate) fn printer_inventory_failure() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "printer_inventory_unavailable",
            "Printer inventory is unavailable.",
        )
    }

    pub(crate) fn profile_catalog_failure() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "profile_catalog_unavailable",
            "The printer profile catalog is unavailable.",
        )
    }

    pub(crate) fn not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "The requested API endpoint was not found.",
        )
    }

    pub(crate) fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "This API endpoint only accepts GET and HEAD requests.",
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

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status;
        let mut response = (
            self.status,
            [(header::CACHE_CONTROL, "no-store")],
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response();
        if status == StatusCode::METHOD_NOT_ALLOWED {
            response
                .headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        }
        response
    }
}

pub(crate) async fn not_found() -> ApiError {
    ApiError::not_found()
}

pub(crate) async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed()
}
