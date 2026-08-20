//! HTTP adapter for `printers add`.
//!
//! This is the first write endpoint the workbench exposes: every other
//! route in this API only reads facts back. Every rule that decides
//! whether a registration is acceptable lives in `add::Request::new` and
//! `add::execute` — a blank name, a blank profile, a blank USB serial, port
//! zero, an out-endpoint outside `0x01..=0x0f`, an in-endpoint outside
//! `0x81..=0x8f`, a colliding name — so the browser can never register a
//! printer the CLI would have refused. This adapter only translates shapes
//! and translates the resulting `ApplicationError` into a stable HTTP code.

use axum::extract::rejection::JsonRejection;
use axum::http::{HeaderName, StatusCode, header};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::web::WebState;
use crate::web::error::ApiError;

use super::AMBIGUOUS_USB_WARNING;

pub(crate) fn router() -> Router<WebState> {
    Router::new().route(
        "/api/printers/add",
        post(add_printer).fallback(crate::web::error::method_not_allowed_post),
    )
}

/// The request body's connection shape mirrors `printers::http::ConnectionResponse`'s
/// field names and `type` tag, so a client that has already parsed a
/// discovered printer's connection recognizes this one instead of learning a
/// third spelling of the same facts. It differs where it must: registration
/// chooses one OUT/IN endpoint pair rather than reporting every endpoint the
/// device exposes, and carries no bus-address or descriptor-string facts,
/// which are read-only observations that have no place in a request.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum AddConnectionBody {
    Usb {
        vendor_id: u16,
        product_id: u16,
        serial_number: Option<String>,
        interface_number: u8,
        out_endpoint: u8,
        in_endpoint: Option<u8>,
    },
    Network {
        host: String,
        port: u16,
    },
}

impl AddConnectionBody {
    /// A USB connection submitted without a serial number is ambiguous on
    /// its face: the vendor/product descriptor is all a client had to name
    /// it by, and that descriptor cannot distinguish this device from
    /// another unit of the same make and model. Unlike the CLI's own
    /// `ambiguous_without_serial`, which compares a chosen device against
    /// every other device connected at prompt time, this handler never sees
    /// a candidate list — the browser already chose one connection — so it
    /// has no sibling device to compare against and warns on the absence of
    /// a serial number alone.
    fn ambiguous_without_serial(&self) -> bool {
        matches!(
            self,
            Self::Usb {
                serial_number: None,
                ..
            }
        )
    }
}

impl From<AddConnectionBody> for super::Connection {
    fn from(connection: AddConnectionBody) -> Self {
        match connection {
            AddConnectionBody::Usb {
                vendor_id,
                product_id,
                serial_number,
                interface_number,
                out_endpoint,
                in_endpoint,
            } => Self::Usb {
                vendor_id,
                product_id,
                serial_number,
                interface_number,
                out_endpoint,
                in_endpoint,
            },
            AddConnectionBody::Network { host, port } => Self::Network { host, port },
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddRequestBody {
    name: String,
    profile: Option<String>,
    connection: AddConnectionBody,
}

#[derive(Serialize)]
struct AddResponseBody {
    name: String,
    transport: &'static str,
    profile: Option<String>,
    warnings: Vec<String>,
}

async fn add_printer(
    body: Result<Json<AddRequestBody>, JsonRejection>,
) -> Result<
    (
        StatusCode,
        [(HeaderName, &'static str); 1],
        Json<AddResponseBody>,
    ),
    ApiError,
> {
    let Json(body) = body.map_err(|_| ApiError::invalid_request_body())?;
    let ambiguous = body.connection.ambiguous_without_serial();
    let request = super::Request::new(None, body.name, body.profile, body.connection.into())
        .map_err(ApiError::from_application)?;
    let response = super::execute(request).map_err(ApiError::from_application)?;

    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(AddResponseBody {
            name: response.printer_name,
            transport: response.connection.transport(),
            profile: response.profile,
            warnings: if ambiguous {
                vec![AMBIGUOUS_USB_WARNING.to_owned()]
            } else {
                Vec::new()
            },
        }),
    ))
}
