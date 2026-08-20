//! HTTP adapter for `printers discover`.
//!
//! The browser's scan-options panel needs to know which networks it can scan
//! before any scan starts, including adapters skipped for being larger than
//! the automatic sweep will cover, so a user can add those as a custom
//! subnet instead of wondering why nothing appeared.
//!
//! The scan itself answers as a `text/event-stream`, because results and
//! progress arrive over seconds and the browser must render them as they
//! land rather than after the sweep ends.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use axum::extract::Query;
use axum::extract::rejection::QueryRejection;
use axum::http::{HeaderValue, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_core::Stream;
use serde::Serialize;

use super::super::cli::{DiscoverPrintersArgs, InventoryTransport};
use super::super::http::ConnectionResponse;
use super::super::inventory::{NusbInventory, UsbEnumerationFailure, UsbFailureStage};
use super::super::list::{ConnectionFacts, NetworkConnectionFacts, UsbConnectionFacts};
use super::{DiscoveryEvent, DiscoveryScope, NetworkDiscovery, UsbDiscovery};
use crate::application::ApplicationError;
use crate::discovery::{self, SkipReason, SkippedInterface, Subnet};
use crate::web::WebState;
use crate::web::error::ApiError;

pub(crate) fn router() -> Router<WebState> {
    Router::new()
        .route(
            "/api/printers/discover",
            get(discover).fallback(crate::web::error::method_not_allowed),
        )
        .route(
            "/api/printers/discover/networks",
            get(networks).fallback(crate::web::error::method_not_allowed),
        )
}

#[derive(Serialize)]
struct NetworksResponse {
    networks: Vec<NetworkResponse>,
    skipped: Vec<SkippedResponse>,
    default_port: u16,
    default_timeout_ms: u64,
}

#[derive(Serialize)]
struct NetworkResponse {
    subnet: String,
    interface: Option<String>,
    hosts: u64,
}

#[derive(Serialize)]
struct SkippedResponse {
    interface: String,
    subnet: Option<String>,
    reason: &'static str,
    /// The shared layer's reason for this omission, in the words every
    /// interface reports it with. The remedy is not included: the terminal
    /// names `--subnet` (`SkippedInterface::cli_hint`), while the browser
    /// points at its own custom-network field.
    description: String,
}

async fn networks() -> Result<
    (
        [(axum::http::HeaderName, &'static str); 1],
        Json<NetworksResponse>,
    ),
    ApiError,
> {
    let addresses = discovery::local_interface_addresses()
        .map_err(|_| ApiError::network_detection_failure())?;
    let (targets, skipped) = discovery::detect_networks(addresses);

    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(NetworksResponse {
            networks: targets.iter().map(network_response).collect(),
            skipped: skipped.iter().map(skipped_response).collect(),
            default_port: DEFAULT_PORT,
            default_timeout_ms: DEFAULT_TIMEOUT_MS,
        }),
    ))
}

/// The CLI's own defaults for the flags this endpoint mirrors, restated once
/// so the panel and the scan agree with `printers discover`.
const DEFAULT_PORT: u16 = 9100;
const DEFAULT_TIMEOUT_MS: u64 = 1000;

fn network_response(target: &discovery::ScanTarget) -> NetworkResponse {
    NetworkResponse {
        subnet: target.subnet.to_string(),
        interface: target.interface.clone(),
        hosts: discovery::probe_count(std::slice::from_ref(target)),
    }
}

fn skipped_response(adapter: &SkippedInterface) -> SkippedResponse {
    SkippedResponse {
        interface: adapter.name.clone(),
        subnet: adapter.subnet.map(|subnet| subnet.to_string()),
        reason: match adapter.reason {
            SkipReason::TooLarge => "too_large",
            SkipReason::UnusableNetmask => "unusable_netmask",
        },
        description: adapter.describe(),
    }
}

/// Events queued by the discovery observer, drained by the stream between
/// polls of the scan future.
type Queue = Arc<Mutex<VecDeque<Event>>>;

/// A discovery scan rendered as a stream of server-sent events.
///
/// The stream owns the scan future rather than spawning it. That ownership
/// is the entire cancellation mechanism: when the browser disconnects, axum
/// drops the response body, which drops this stream, which drops the future,
/// which drops the `JoinSet` inside `discovery::scan` and aborts every
/// outstanding probe. A `tokio::spawn` here would detach the sweep from the
/// request and leave it grinding through hundreds of addresses for a client
/// that has already left.
struct DiscoveryStream {
    queue: Queue,
    scan: Pin<Box<dyn Future<Output = ()> + Send>>,
    scan_done: bool,
}

impl Stream for DiscoveryStream {
    type Item = Result<Event, Infallible>;

    /// Drive the scan, then hand back one event it queued. The observer is
    /// synchronous and can queue several events within a single poll of the
    /// scan, so the queue is drained one event per poll — returning `Ready`
    /// means the consumer polls again, and the scan is simply polled again
    /// too on its way to the next queued event.
    ///
    /// A wakeup cannot be lost. Every push happens synchronously inside the
    /// scan future while this call has it polled, so the queue can only be
    /// found empty when the scan itself returned `Pending`, having already
    /// registered `context`'s waker with whatever it is waiting on. There is
    /// no window in which an event is queued by something this poll did not
    /// drive.
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if !this.scan_done && this.scan.as_mut().poll(context).is_ready() {
            this.scan_done = true;
        }
        let next = this
            .queue
            .lock()
            .expect("the discovery queue is never poisoned")
            .pop_front();
        match next {
            Some(event) => Poll::Ready(Some(Ok(event))),
            None if this.scan_done => Poll::Ready(None),
            None => Poll::Pending,
        }
    }
}

async fn discover(
    query: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(parameters) = query.map_err(|_| ApiError::invalid_query())?;
    let scope = scope(&parameters)?;
    let prepared = super::prepare(None, scope).map_err(rejected_discovery)?;

    let queue: Queue = Arc::new(Mutex::new(VecDeque::new()));
    let sink = Arc::clone(&queue);
    let scan = Box::pin(async move {
        let mut inventory = NusbInventory;
        let outcome = super::execute(
            prepared,
            |event| {
                // Encoded before the lock is taken: a panic while
                // serializing would otherwise poison the queue and take the
                // stream down with it.
                let message = encode(event);
                sink.lock()
                    .expect("the discovery queue is never poisoned")
                    .push_back(message);
            },
            &mut inventory,
        )
        .await;
        // The operation's final `super::Response` adds nothing the stream
        // has not already reported — every printer arrived as its own
        // event — so the closing event is a bare end-of-stream marker.
        let closing = match outcome {
            Ok(_) => Event::default().event("completed").data("{}"),
            Err(error) => Event::default()
                .event("error")
                .data(serde_json::json!({ "message": error.to_string() }).to_string()),
        };
        sink.lock()
            .expect("the discovery queue is never poisoned")
            .push_back(closing);
    });

    let mut response = Sse::new(DiscoveryStream {
        queue,
        scan,
        scan_done: false,
    })
    .keep_alive(KeepAlive::default())
    .into_response();
    // `Sse` sets `cache-control: no-cache`; every other response in this API
    // says `no-store`, and each route's test asserts it. Inserted on the
    // finished response rather than returned as a header tuple, which would
    // append a second value instead of replacing the first.
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

/// Why `prepare` refused, in HTTP terms. A subnet too large to scan is the
/// caller's mistake and the caller can fix it, so it is a bad request; an
/// unreadable configuration or an unenumerable interface list is the
/// server's problem.
fn rejected_discovery(error: ApplicationError) -> ApiError {
    match error {
        ApplicationError::SubnetTooLargeToScan(_) => ApiError::invalid_query(),
        _ => ApiError::discovery_failure(),
    }
}

/// The requested scope, built by handing the query parameters to the CLI's
/// own flag rules. Going through `DiscoverPrintersArgs` keeps the web app
/// from ever accepting an input `printers discover` does not, and shares one
/// definition of which flag combinations are contradictory.
fn scope(parameters: &[(String, String)]) -> Result<DiscoveryScope, ApiError> {
    let mut arguments = DiscoverPrintersArgs {
        transport: None,
        port: None,
        subnet: Vec::new(),
        timeout: None,
    };
    // `serde_urlencoded` cannot deserialize a repeated key into a `Vec`
    // field, and `subnet` repeats, so the pairs are walked by hand. An
    // unknown key is rejected rather than ignored, matching the derived
    // `deny_unknown_fields` queries elsewhere in this API.
    for (name, value) in parameters {
        match name.as_str() {
            "transport" => set_once(
                &mut arguments.transport,
                match value.as_str() {
                    "usb" => InventoryTransport::Usb,
                    "network" => InventoryTransport::Network,
                    _ => return Err(ApiError::invalid_query()),
                },
            )?,
            "subnet" => arguments
                .subnet
                .push(Subnet::parse(value).map_err(|_| ApiError::invalid_query())?),
            "port" => set_once(
                &mut arguments.port,
                value.parse().map_err(|_| ApiError::invalid_query())?,
            )?,
            "timeout" => set_once(
                &mut arguments.timeout,
                value.parse().map_err(|_| ApiError::invalid_query())?,
            )?,
            _ => return Err(ApiError::invalid_query()),
        }
    }
    DiscoveryScope::try_from(arguments).map_err(|_| ApiError::invalid_query())
}

/// Accept a single-valued parameter exactly once. A repeated `port` has no
/// meaning the CLI can express, so it is a bad query rather than a silent
/// last-one-wins.
fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), ApiError> {
    if slot.is_some() {
        return Err(ApiError::invalid_query());
    }
    *slot = Some(value);
    Ok(())
}

/// One discovery event as a server-sent event. Every event the operation
/// layer reports is forwarded: the stream carries facts, and which of them
/// the printers page shows is the page's decision.
///
/// `Prepared` also carries the config path and the scope, which the terminal
/// echoes back to the user. The browser already knows both — it asked for
/// the scope, and the config path is not its business — so neither is sent.
fn encode(event: DiscoveryEvent<'_>) -> Event {
    match event {
        DiscoveryEvent::Prepared {
            scan_targets,
            skipped,
            ..
        } => json_event(
            "prepared",
            &PreparedEvent {
                targets: scan_targets.iter().map(network_response).collect(),
                skipped: skipped.iter().map(skipped_response).collect(),
                total_probes: discovery::probe_count(scan_targets),
            },
        ),
        DiscoveryEvent::UsbPrinter(printer) => json_event("printer", &usb_printer_event(printer)),
        DiscoveryEvent::NetworkPrinter(printer) => {
            json_event("printer", &network_printer_event(printer))
        }
        DiscoveryEvent::UsbFailure(failure) => {
            json_event("usb_failure", &usb_failure_event(failure))
        }
        DiscoveryEvent::NetworkScanProgress { completed, total } => {
            json_event("progress", &ProgressEvent { completed, total })
        }
    }
}

/// Serialization here cannot fail: every payload is a plain struct of
/// strings, numbers, and vectors, with no map keys and no custom
/// `Serialize`, so the fallback empty object is unreachable in practice.
fn json_event(name: &'static str, payload: &impl Serialize) -> Event {
    Event::default()
        .event(name)
        .data(serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_owned()))
}

#[derive(Serialize)]
struct PreparedEvent {
    targets: Vec<NetworkResponse>,
    skipped: Vec<SkippedResponse>,
    total_probes: u64,
}

#[derive(Serialize)]
struct ProgressEvent {
    completed: u64,
    total: u64,
}

/// One discovered printer. `configured_names` is a list for both transports
/// even though a USB printer matches at most one saved entry: the browser
/// renders both kinds from one shape, and a network endpoint really can
/// carry several names.
#[derive(Serialize)]
struct PrinterEvent {
    transport: &'static str,
    configured_names: Vec<String>,
    configured_profile: Option<String>,
    /// The adapter a network host answered on. Absent for USB.
    #[serde(skip_serializing_if = "Option::is_none")]
    interface: Option<String>,
    connection: ConnectionResponse,
}

#[derive(Serialize)]
struct UsbFailureEvent {
    vendor_id: u16,
    product_id: u16,
    stage: &'static str,
    reason: String,
    permission_denied: bool,
    /// Whether this host has `printers grant-usb-permissions` at all — the
    /// subcommand is Linux-only, and only the server knows what it runs on.
    /// A platform fact, not a remedy: the browser still words its own.
    can_grant_usb_permissions: bool,
}

fn usb_printer_event(discovered: &UsbDiscovery) -> PrinterEvent {
    let printer = &discovered.printer;
    PrinterEvent {
        transport: "usb",
        configured_names: discovered.configured_name.clone().into_iter().collect(),
        configured_profile: discovered.configured_profile.clone(),
        interface: None,
        connection: ConnectionResponse::from(ConnectionFacts::Usb(UsbConnectionFacts {
            vendor_id: printer.vendor_id,
            product_id: printer.product_id,
            bus: Some(printer.bus.clone()),
            address: Some(printer.address),
            manufacturer: printer.manufacturer.clone(),
            product: printer.product.clone(),
            serial_number: printer.serial_number.clone(),
            interface_number: printer.interface_number,
            out_endpoints: printer.out_endpoints.clone(),
            in_endpoints: printer.in_endpoints.clone(),
        })),
    }
}

fn network_printer_event(discovered: &NetworkDiscovery) -> PrinterEvent {
    PrinterEvent {
        transport: "network",
        configured_names: discovered.configured_names.clone(),
        configured_profile: discovered.configured_profile.clone(),
        interface: discovered.interface.clone(),
        connection: ConnectionResponse::from(ConnectionFacts::Network(NetworkConnectionFacts {
            host: discovered.host.clone(),
            port: discovered.port,
        })),
    }
}

fn usb_failure_event(failure: &UsbEnumerationFailure) -> UsbFailureEvent {
    UsbFailureEvent {
        vendor_id: failure.vendor_id,
        product_id: failure.product_id,
        stage: match failure.stage {
            UsbFailureStage::OpenDevice => "open_device",
            UsbFailureStage::InspectConfiguration => "inspect_configuration",
        },
        reason: failure.reason.clone(),
        permission_denied: failure.permission_denied,
        can_grant_usb_permissions: cfg!(target_os = "linux"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn advertised_defaults_match_the_command_line_defaults() {
        let scope = scope(&[]).expect("an empty query should discover everything");
        let scan = scope
            .network_scan()
            .expect("the default scope should include a network scan");
        assert_eq!(scan.port(), DEFAULT_PORT);
        assert_eq!(scan.timeout(), Duration::from_millis(DEFAULT_TIMEOUT_MS));
        assert!(scan.uses_automatic_subnets());
    }

    #[test]
    fn a_repeated_single_valued_parameter_is_rejected() {
        let parameters = [
            ("port".to_owned(), "9100".to_owned()),
            ("port".to_owned(), "9101".to_owned()),
        ];
        assert!(scope(&parameters).is_err());
    }

    #[test]
    fn repeated_subnets_all_reach_the_scan() {
        let parameters = [
            ("subnet".to_owned(), "10.42.0.0/24".to_owned()),
            ("subnet".to_owned(), "10.43.0.0/24".to_owned()),
        ];
        let scope = scope(&parameters).expect("two subnets should be accepted");
        let subnets = scope
            .network_scan()
            .expect("an explicit subnet implies a network scan")
            .subnets()
            .iter()
            .map(Subnet::to_string)
            .collect::<Vec<_>>();
        assert_eq!(subnets, ["10.42.0.0/24", "10.43.0.0/24"]);
    }

    #[cfg(target_os = "linux")]
    const GRANTABLE_ON_THIS_PLATFORM: bool = true;
    #[cfg(not(target_os = "linux"))]
    const GRANTABLE_ON_THIS_PLATFORM: bool = false;

    fn open_failure() -> UsbEnumerationFailure {
        UsbEnumerationFailure {
            stage: UsbFailureStage::OpenDevice,
            vendor_id: 0x04b8,
            product_id: 0x0202,
            reason: "permission denied (errno 13)".to_owned(),
            permission_denied: true,
        }
    }

    /// The browser words its own remedy for a refused device, so the event has
    /// to carry every fact that remedy depends on — including whether this
    /// host has the command to suggest at all.
    #[test]
    fn a_usb_failure_event_carries_the_facts_a_remedy_is_worded_from() {
        let payload = serde_json::to_value(usb_failure_event(&open_failure()))
            .expect("the failure event should serialize");

        assert_eq!(
            payload,
            serde_json::json!({
                "vendor_id": 0x04b8,
                "product_id": 0x0202,
                "stage": "open_device",
                "reason": "permission denied (errno 13)",
                "permission_denied": true,
                // `printers grant-usb-permissions` is a Linux-only subcommand
                // and the browser cannot know what the server runs on. Naming
                // it on a macOS host would send the reader to a command that
                // is unrecognized there, while the CLI on that same host says
                // nothing at all.
                //
                // Stated as a literal per platform rather than as the same
                // `cfg!` the source uses: an assertion that recomputes the
                // expression it is checking would accept a hardcoded `true`,
                // which is exactly the bug this field exists to prevent.
                "can_grant_usb_permissions": GRANTABLE_ON_THIS_PLATFORM,
            })
        );
    }

    /// A failure that is not a permission problem stays one: nothing about the
    /// stage may leak into the permission flag, or the browser suggests a
    /// remedy for a disconnected device.
    #[test]
    fn an_inspection_failure_reports_its_own_stage_without_claiming_permission() {
        let payload = serde_json::to_value(usb_failure_event(&UsbEnumerationFailure {
            stage: UsbFailureStage::InspectConfiguration,
            permission_denied: false,
            reason: "device disconnected".to_owned(),
            ..open_failure()
        }))
        .expect("the failure event should serialize");

        assert_eq!(payload["stage"], "inspect_configuration");
        assert_eq!(payload["permission_denied"], false);
        assert_eq!(payload["reason"], "device disconnected");
    }
}
