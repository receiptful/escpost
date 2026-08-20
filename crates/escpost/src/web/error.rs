use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    /// The `Allow` header value for a 405, e.g. `"GET, HEAD"`. Only ever set
    /// by `method_not_allowed`, which is the only constructor that produces
    /// `StatusCode::METHOD_NOT_ALLOWED`.
    allow: Option<String>,
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

    /// The request body was not JSON, or did not match the expected shape.
    /// Raised in place of axum's built-in `JsonRejection` response, which
    /// speaks `text/plain` rather than this API's error envelope.
    pub(crate) fn invalid_request_body() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "invalid_request_body",
            "The request body is invalid.",
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

    pub(crate) fn network_detection_failure() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "network_detection_unavailable",
            "The machine's network interfaces could not be read.",
        )
    }

    /// Discovery could not even be prepared — the configuration is unreadable
    /// or the requested scope leaves nothing to scan. Raised before the stream
    /// opens, so the browser gets a plain JSON error rather than an event
    /// stream whose first event is a failure.
    pub(crate) fn discovery_failure() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "discovery_unavailable",
            "Printer discovery could not be started.",
        )
    }

    pub(crate) fn not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "The requested API endpoint was not found.",
        )
    }

    /// `methods` are the methods the route's own handlers actually accept
    /// (e.g. `&["GET", "HEAD"]` or `&["POST"]`), so the message and the
    /// `Allow` header stay truthful per route instead of a single sentence
    /// that was only ever true back when every route was a GET.
    pub(crate) fn method_not_allowed(methods: &[&str]) -> Self {
        Self {
            status: StatusCode::METHOD_NOT_ALLOWED,
            code: "method_not_allowed",
            message: format!(
                "This API endpoint only accepts {} requests.",
                describe_methods(methods)
            ),
            allow: Some(methods.join(", ")),
        }
    }

    pub(crate) fn job_not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "job_not_found",
            "The requested print job is no longer available.",
        )
    }

    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            allow: None,
        }
    }

    /// Translate a failure from `add::Request::new` or `add::execute` into
    /// its own stable code, one `ApplicationError` variant at a time, so the
    /// browser can tell a name collision (409) from a bad endpoint (400)
    /// instead of parsing prose out of a single generic 400.
    pub(crate) fn from_application(error: crate::application::ApplicationError) -> Self {
        use crate::application::ApplicationError as Application;
        let (status, code) = match error {
            Application::BlankPrinterName => (StatusCode::BAD_REQUEST, "blank_printer_name"),
            Application::BlankPrinterHost => (StatusCode::BAD_REQUEST, "blank_printer_host"),
            Application::BlankPrinterProfile => (StatusCode::BAD_REQUEST, "blank_printer_profile"),
            Application::BlankUsbSerialNumber => {
                (StatusCode::BAD_REQUEST, "blank_usb_serial_number")
            }
            Application::InvalidPrinterPort => (StatusCode::BAD_REQUEST, "invalid_printer_port"),
            Application::InvalidUsbOutEndpoint(_) => {
                (StatusCode::BAD_REQUEST, "invalid_usb_out_endpoint")
            }
            Application::InvalidUsbInEndpoint(_) => {
                (StatusCode::BAD_REQUEST, "invalid_usb_in_endpoint")
            }
            Application::PrinterAlreadyConfigured(_) => {
                (StatusCode::CONFLICT, "printer_already_configured")
            }
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "printer_registration_failed",
            ),
        };
        Self::new(status, code, error.to_string())
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
        if status == StatusCode::METHOD_NOT_ALLOWED
            && let Some(allow) = &self.allow
        {
            response.headers_mut().insert(
                header::ALLOW,
                HeaderValue::from_str(allow)
                    .expect("method names are valid header value characters"),
            );
        }
        response
    }
}

/// English-join method names for the 405 message, e.g. `["POST"] ->
/// "POST"` and `["GET", "HEAD"] -> "GET and HEAD"`.
fn describe_methods(methods: &[&str]) -> String {
    match methods {
        [] => String::new(),
        [only] => (*only).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

pub(crate) async fn not_found() -> ApiError {
    ApiError::not_found()
}

/// Fallback for every route whose only handlers are GET (with axum's
/// implicit HEAD).
pub(crate) async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed(&["GET", "HEAD"])
}

/// Fallback for `POST /api/printers/add`, the one write route: it has no
/// GET handler, so the shared GET/HEAD message would lie about what this
/// route accepts.
pub(crate) async fn method_not_allowed_post() -> ApiError {
    ApiError::method_not_allowed(&["POST"])
}
