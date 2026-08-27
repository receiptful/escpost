use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::{HeaderValue, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use tokio::sync::mpsc;

use crate::web::WebState;
use crate::web::error::ApiError;

use super::list::{self, ConnectionFacts, Printer};
use super::monitor;
use super::{Availability, Transport};

pub(crate) fn router() -> Router<WebState> {
    Router::new()
        .route(
            "/api/printers/list",
            get(list_printers).fallback(crate::web::error::method_not_allowed),
        )
        .route(
            "/api/printers/list/events",
            get(list_printer_events).fallback(crate::web::error::method_not_allowed),
        )
        .merge(super::discover::http::router())
        .merge(super::add::http::router())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    transport: Option<HttpTransport>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsQuery {}

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
    let snapshot = monitor::collect_once(list::Request {
        config: None,
        transport: query.transport.map(transport),
    })
    .await
    .map_err(|_| ApiError::printer_inventory_failure())?;
    let response = ListResponse::try_from(snapshot)
        .expect("a UTC inventory snapshot should always format as RFC 3339");

    Ok(([(header::CACHE_CONTROL, "no-store")], Json(response)))
}

struct PrinterStream {
    receiver: mpsc::Receiver<Result<Event, Infallible>>,
}

impl Stream for PrinterStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(context)
    }
}

async fn list_printer_events(
    State(state): State<WebState>,
    query: Result<Query<EventsQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(_) = query.map_err(|_| ApiError::invalid_query())?;
    let mut subscription = state.printer_monitor.subscribe();
    let (sender, event_receiver) = mpsc::channel(1);
    tokio::spawn(async move {
        loop {
            let snapshot = tokio::select! {
                _ = sender.closed() => break,
                snapshot = subscription.next() => snapshot,
            };
            let Some(snapshot) = snapshot else {
                break;
            };
            let response = ListResponse::try_from(snapshot)
                .expect("a UTC inventory snapshot should always format as RFC 3339");
            if sender
                .send(Ok(Event::default()
                    .data(serde_json::to_string(&response).expect(
                        "printer inventory snapshots contain only serializable fields",
                    ))))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let mut response = Sse::new(PrinterStream {
        receiver: event_receiver,
    })
    .keep_alive(KeepAlive::default())
    .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn transport(transport: HttpTransport) -> Transport {
    match transport {
        HttpTransport::Usb => Transport::Usb,
        HttpTransport::Network => Transport::Network,
    }
}

#[derive(Serialize)]
struct ListResponse {
    updated_at: String,
    warning: Option<String>,
    printers: Vec<PrinterResponse>,
}

impl TryFrom<monitor::Snapshot> for ListResponse {
    type Error = time::error::Format;

    fn try_from(snapshot: monitor::Snapshot) -> Result<Self, Self::Error> {
        Ok(Self {
            updated_at: snapshot.updated_at.format(&Rfc3339)?,
            warning: snapshot.warning,
            printers: snapshot
                .printers
                .into_iter()
                .map(PrinterResponse::from)
                .collect(),
        })
    }
}

#[derive(Serialize)]
struct PrinterResponse {
    name: String,
    transport: &'static str,
    availability: &'static str,
    profile: Option<String>,
    connection: ConnectionResponse,
}

/// The one connection shape the printers API speaks, shared by the listing
/// here and by the discovery stream's `printer` events so a client parses a
/// printer's connection the same way wherever it arrives.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ConnectionResponse {
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
