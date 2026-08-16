use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::header;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::web::WebState;
use crate::web::error::ApiError;

use super::list::{self, ConnectionFacts, Printer};
use super::{Availability, Transport};

pub(crate) fn router() -> Router<WebState> {
    Router::new().route(
        "/api/printers/list",
        get(list_printers).fallback(crate::web::error::method_not_allowed),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    transport: Option<HttpTransport>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum HttpTransport {
    Usb,
    Network,
}

async fn list_printers(
    State(_state): State<WebState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<
    (
        [(axum::http::HeaderName, &'static str); 1],
        Json<ListResponse>,
    ),
    ApiError,
> {
    let Query(query) = query.map_err(|_| ApiError::invalid_query())?;
    let response = list::execute_with_observer(
        list::Request {
            config: None,
            transport: query.transport.map(transport),
        },
        |_| {},
    )
    .await
    .map_err(|_| ApiError::printer_inventory_failure())?;

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(ListResponse {
            printers: response
                .printers
                .into_iter()
                .map(PrinterResponse::from)
                .collect(),
        }),
    ))
}

fn transport(transport: HttpTransport) -> Transport {
    match transport {
        HttpTransport::Usb => Transport::Usb,
        HttpTransport::Network => Transport::Network,
    }
}

#[derive(Serialize)]
struct ListResponse {
    printers: Vec<PrinterResponse>,
}

#[derive(Serialize)]
struct PrinterResponse {
    name: String,
    transport: &'static str,
    availability: &'static str,
    profile: Option<String>,
    connection: ConnectionResponse,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ConnectionResponse {
    Usb {
        vendor_id: u16,
        product_id: u16,
        bus: Option<String>,
        address: Option<u8>,
        manufacturer: Option<String>,
        product: Option<String>,
        serial_number: Option<String>,
        interface_number: u8,
        out_endpoints: Vec<u8>,
        in_endpoints: Vec<u8>,
    },
    Network {
        host: String,
        port: u16,
    },
}

impl From<Printer> for PrinterResponse {
    fn from(printer: Printer) -> Self {
        Self {
            name: printer.name,
            transport: transport_label(printer.transport),
            availability: availability_label(printer.availability),
            profile: printer.profile,
            connection: ConnectionResponse::from(printer.connection),
        }
    }
}

impl From<ConnectionFacts> for ConnectionResponse {
    fn from(connection: ConnectionFacts) -> Self {
        match connection {
            ConnectionFacts::Usb(connection) => Self::Usb {
                vendor_id: connection.vendor_id,
                product_id: connection.product_id,
                bus: connection.bus,
                address: connection.address,
                manufacturer: connection.manufacturer,
                product: connection.product,
                serial_number: connection.serial_number,
                interface_number: connection.interface_number,
                out_endpoints: connection.out_endpoints,
                in_endpoints: connection.in_endpoints,
            },
            ConnectionFacts::Network(connection) => Self::Network {
                host: connection.host,
                port: connection.port,
            },
        }
    }
}

fn transport_label(transport: Transport) -> &'static str {
    match transport {
        Transport::Usb => "usb",
        Transport::Network => "network",
    }
}

fn availability_label(availability: Availability) -> &'static str {
    match availability {
        Availability::Connected => "connected",
        Availability::Unavailable => "unavailable",
    }
}
