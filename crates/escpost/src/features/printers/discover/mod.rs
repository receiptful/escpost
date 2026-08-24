//! Structured USB and network discovery operation.

use std::num::NonZeroU16;
use std::path::PathBuf;
use std::time::Duration;

use crate::application::{self, ApplicationError};
use crate::configuration::{self, PrinterConfiguration};
use crate::discovery::{
    self, DiscoveredHost, InterfaceAddress, ScanTarget, SharedProber, SkippedInterface, Subnet,
};

use super::inventory::{
    UsbEnumeration, UsbEnumerationFailure, UsbInventory, UsbPrinter, classify_usb_printers,
    sort_by_usb_location,
};

pub(crate) mod cli;
pub(crate) mod http;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DiscoveryScope {
    Usb,
    Network(NetworkScan),
    All(NetworkScan),
}

impl DiscoveryScope {
    pub(crate) fn network_scan(&self) -> Option<&NetworkScan> {
        match self {
            Self::Usb => None,
            Self::Network(scan) | Self::All(scan) => Some(scan),
        }
    }

    fn includes_usb(&self) -> bool {
        matches!(self, Self::Usb | Self::All(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NetworkScan {
    port: NonZeroU16,
    subnets: Vec<Subnet>,
    timeout: Duration,
}

impl NetworkScan {
    pub(crate) fn new(
        port: u16,
        subnets: Vec<Subnet>,
        timeout: Duration,
    ) -> application::Result<Self> {
        let port = NonZeroU16::new(port).ok_or(ApplicationError::InvalidPrinterPort)?;
        Ok(Self {
            port,
            subnets,
            timeout,
        })
    }

    pub(crate) fn port(&self) -> u16 {
        self.port.get()
    }

    pub(crate) fn subnets(&self) -> &[Subnet] {
        &self.subnets
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) fn uses_automatic_subnets(&self) -> bool {
        self.subnets.is_empty()
    }
}

pub(crate) struct PreparedDiscovery {
    configuration: PrinterConfiguration,
    config_path: PathBuf,
    scope: DiscoveryScope,
    scan_targets: Vec<ScanTarget>,
    skipped: Vec<SkippedInterface>,
    /// How the sweep tests each address. `prepare` always puts the real TCP
    /// prober here, so production code names no prober at all.
    prober: SharedProber,
}

impl PreparedDiscovery {
    pub(crate) fn skipped(&self) -> &[SkippedInterface] {
        &self.skipped
    }

    /// Replace the prober. A test uses this to sweep without a socket: the
    /// scan then reports what the given prober says, and touches no address.
    #[cfg(test)]
    fn with_prober(mut self, prober: SharedProber) -> Self {
        self.prober = prober;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum DiscoveryEvent<'a> {
    Prepared {
        config_path: &'a std::path::Path,
        scope: &'a DiscoveryScope,
        scan_targets: &'a [ScanTarget],
        skipped: &'a [SkippedInterface],
    },
    UsbPrinter(&'a UsbDiscovery),
    UsbFailure(&'a UsbEnumerationFailure),
    /// One classified network host, fired as `discovery::scan` finds it.
    /// `discovery::ScanEvent::Found` fires at most once per address even
    /// when scan targets overlap (see its own doc comment), so this fires
    /// at most once per host too: the set of hosts announced this way
    /// equals the set in the final `Response.network_printers`.
    NetworkPrinter(&'a NetworkDiscovery),
    NetworkScanProgress {
        completed: u64,
        total: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Response {
    pub(crate) config_path: PathBuf,
    pub(crate) scan_targets: Vec<ScanTarget>,
    pub(crate) usb_printers: Vec<UsbDiscovery>,
    pub(crate) network_printers: Vec<NetworkDiscovery>,
    pub(crate) usb_failures: Vec<UsbEnumerationFailure>,
    pub(crate) registration: RegistrationAvailability,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UsbDiscovery {
    pub(crate) configured_name: Option<String>,
    pub(crate) configured_profile: Option<String>,
    pub(crate) printer: UsbPrinter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NetworkDiscovery {
    pub(crate) configured_names: Vec<String>,
    pub(crate) configured_profile: Option<String>,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) interface: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RegistrationAvailability {
    pub(crate) usb: bool,
    pub(crate) network: bool,
}

pub(crate) fn prepare(
    config: Option<PathBuf>,
    scope: DiscoveryScope,
) -> application::Result<PreparedDiscovery> {
    let configuration = configuration::load_for_update(config.as_deref())?;
    let config_path = configuration::resolved_path(config.as_deref())?;
    let (scan_targets, skipped) = match scope.network_scan() {
        Some(scan) => {
            let (targets, skipped) = resolve_targets(scan.subnets())?;
            if targets.is_empty()
                && let Some(error) = empty_targets_error(&scope, &skipped)
            {
                return Err(error);
            }
            (targets, skipped)
        }
        None => (Vec::new(), Vec::new()),
    };
    Ok(PreparedDiscovery {
        configuration,
        config_path,
        scope,
        scan_targets,
        skipped,
        prober: discovery::tcp_prober(),
    })
}

/// Whether an empty automatic sweep is fatal, and the error to report if so.
/// A network-only scope has no fallback: zero targets means zero work, so it
/// fails. A combined scope still has USB to enumerate, so an empty network
/// sweep is reported through `skipped` instead of aborting the command.
fn empty_targets_error(
    scope: &DiscoveryScope,
    skipped: &[SkippedInterface],
) -> Option<ApplicationError> {
    if scope.includes_usb() {
        return None;
    }
    let detail = discovery::describe_skipped(skipped);
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!(": {detail}")
    };
    Some(ApplicationError::NoDiscoverableSubnets(detail))
}

// `pub(in crate::features::printers)` rather than `pub(crate)`: the
// `UsbInventory` bound below is itself only nameable within
// `features::printers` (see `inventory.rs`), and both real callers —
// `discover::cli` and `add::cli` — already live inside that subtree.
pub(in crate::features::printers) async fn execute(
    prepared: PreparedDiscovery,
    mut observer: impl FnMut(DiscoveryEvent<'_>),
    inventory: &mut impl UsbInventory,
) -> application::Result<Response> {
    observer(DiscoveryEvent::Prepared {
        config_path: &prepared.config_path,
        scope: &prepared.scope,
        scan_targets: &prepared.scan_targets,
        skipped: prepared.skipped(),
    });

    // USB is enumerated, and its events emitted, before the network sweep
    // starts: a caller watching for results sees USB printers immediately
    // rather than waiting behind a sweep that may take seconds.
    let enumeration = if prepared.scope.includes_usb() {
        inventory.list_tolerant()?
    } else {
        UsbEnumeration {
            printers: Vec::new(),
            failures: Vec::new(),
        }
    };
    let mut printers = enumeration.printers;
    sort_by_usb_location(&mut printers);
    let connected = classify_usb_printers(printers, &prepared.configuration).0;
    let usb_printers = connected
        .into_iter()
        .map(|connected| {
            let configured = connected
                .configuration_index
                .map(|index| &prepared.configuration.usb_printers()[index]);
            UsbDiscovery {
                configured_name: configured.map(|printer| printer.name.clone()),
                configured_profile: configured.and_then(|printer| printer.profile.clone()),
                printer: connected.printer,
            }
        })
        .collect::<Vec<_>>();
    for printer in &usb_printers {
        observer(DiscoveryEvent::UsbPrinter(printer));
    }
    for failure in &enumeration.failures {
        observer(DiscoveryEvent::UsbFailure(failure));
    }

    // `discovery::scan` reports each host as it answers via `Found`, ahead
    // of its own return value, so a live caller can render results while
    // the sweep is still running. The final `network_printers` list below
    // is still built from the returned, deduplicated hosts (a host can be
    // probed twice when explicit --subnet values overlap; see `scan`'s own
    // doc comment) so the assembled `Response` matches what a non-streaming
    // caller would have produced.
    let hosts = if let Some(scan) = prepared.scope.network_scan() {
        discovery::scan(
            &prepared.scan_targets,
            scan.port(),
            scan.timeout(),
            &prepared.prober,
            |event| match event {
                discovery::ScanEvent::Progress { completed, total } => {
                    observer(DiscoveryEvent::NetworkScanProgress { completed, total });
                }
                discovery::ScanEvent::Found(host) => {
                    let printer = classify_network_host(&prepared.configuration, host);
                    observer(DiscoveryEvent::NetworkPrinter(&printer));
                }
            },
        )
        .await
    } else {
        Vec::new()
    };
    let network_printers = hosts
        .iter()
        .map(|host| classify_network_host(&prepared.configuration, host))
        .collect::<Vec<_>>();

    let registration = RegistrationAvailability {
        usb: usb_printers
            .iter()
            .any(|printer| printer.configured_name.is_none()),
        network: network_printers
            .iter()
            .any(|printer| printer.configured_names.is_empty()),
    };
    Ok(Response {
        config_path: prepared.config_path,
        scan_targets: prepared.scan_targets,
        usb_printers,
        network_printers,
        usb_failures: enumeration.failures,
        registration,
    })
}

/// One network host's classification, shared by the live `Found` event and
/// the final `Response` assembly in `execute` so the two never drift apart.
fn classify_network_host(
    configuration: &PrinterConfiguration,
    host: &DiscoveredHost,
) -> NetworkDiscovery {
    NetworkDiscovery {
        configured_names: configured_names(configuration, host),
        configured_profile: configuration
            .network_printers()
            .iter()
            .find(|printer| printer.port == host.port && printer.host == host.address.to_string())
            .and_then(|printer| printer.profile.clone()),
        host: host.address.to_string(),
        port: host.port,
        interface: host.interface.clone(),
    }
}

#[cfg(test)]
async fn build_response(
    config: Option<PathBuf>,
    scope: DiscoveryScope,
    inventory: &mut impl UsbInventory,
) -> application::Result<Response> {
    execute(prepare(config, scope)?, |_| {}, inventory).await
}

/// Targets for the given subnets, or for every automatically detected one
/// when `subnets` is empty. An empty result is not an error here: whether
/// zero targets is fatal depends on the discovery scope, which only `prepare`
/// knows, so that decision lives there instead.
pub(crate) fn resolve_targets(
    subnets: &[Subnet],
) -> application::Result<(Vec<ScanTarget>, Vec<SkippedInterface>)> {
    let addresses = discovery::local_interface_addresses()?;
    resolve_targets_from(subnets, addresses)
}

/// The pure half of `resolve_targets`, split out so automatic detection can
/// be tested against a chosen `InterfaceAddress` list instead of this
/// machine's real interfaces. Fails only for a named subnet too large to
/// scan; automatic detection reports its omissions through `skipped` and
/// never errors here.
fn resolve_targets_from(
    subnets: &[Subnet],
    addresses: Vec<InterfaceAddress>,
) -> application::Result<(Vec<ScanTarget>, Vec<SkippedInterface>)> {
    if subnets.is_empty() {
        return Ok(discovery::detect_networks(addresses));
    }
    Ok((
        discovery::explicit_scan_targets(subnets, &addresses)?,
        Vec::new(),
    ))
}

fn configured_names(configuration: &PrinterConfiguration, host: &DiscoveredHost) -> Vec<String> {
    configuration
        .network_printers()
        .iter()
        .filter(|printer| printer.port == host.port && printer.host == host.address.to_string())
        .map(|printer| printer.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{ProbeFuture, Prober, ScanTarget, SharedProber, SkipReason, Subnet};
    use crate::features::printers::inventory::{
        UsbEnumeration, UsbEnumerationFailure, UsbFailureStage,
    };
    use crate::features::printers::test_support::{
        TolerantInventory, netum_usb_printer, temporary_configuration,
    };
    use std::net::{Ipv4Addr, TcpListener};
    use std::sync::Arc;
    use std::time::Duration;

    /// A prober that opens no socket and answers "no listener" for every
    /// address. A test that pairs it with `PreparedDiscovery::with_prober`
    /// sweeps a subnet without touching a single address.
    ///
    /// The tests must pass on every machine. A test that connects to an
    /// address it does not own cannot promise that. Reserved ranges do not
    /// help: this suite has seen answers from both TEST-NET-1 and TEST-NET-3
    /// on a development machine, because a VPN or a route can put a real
    /// host behind any address. Only a prober the test controls removes the
    /// risk.
    struct NoListeners;

    impl Prober for NoListeners {
        fn probe(&self, _address: Ipv4Addr, _port: u16, _probe_timeout: Duration) -> ProbeFuture {
            Box::pin(std::future::ready(false))
        }
    }

    fn no_listeners() -> SharedProber {
        Arc::new(NoListeners)
    }

    /// A scan of a subnet that no probe ever opens. Of the tests that use
    /// it, only one sweeps, and that one supplies `no_listeners`; the others
    /// stop at `prepare` or at an error. The CIDR is therefore only the
    /// shape of the work — one /24 — and not an address this suite touches.
    /// Give this scan the real prober and the rule at the top of
    /// `no_listeners` is broken again.
    fn explicit_network_scan() -> NetworkScan {
        NetworkScan::new(
            9100,
            vec![Subnet::parse("203.0.113.0/24").expect("valid subnet")],
            Duration::from_millis(50),
        )
        .expect("the explicit network scan should be valid")
    }

    /// Bind a real loopback listener that stands in for a network printer,
    /// and hand back the ephemeral port it bound. 127.0.0.1 is this
    /// machine's own address and gets self-excluded by
    /// `explicit_scan_targets`, so tests that need a real discovered host
    /// use a different loopback address instead — the same trick
    /// `features::printers::add::cli`'s own discovery test relies on.
    fn loopback_listener(address: [u8; 4]) -> (TcpListener, u16) {
        let listener = TcpListener::bind((Ipv4Addr::from(address), 0))
            .expect("an ephemeral loopback port should bind");
        let port = listener
            .local_addr()
            .expect("the listener should report its address")
            .port();
        (listener, port)
    }

    /// A `NetworkScan` naming exactly one CIDR, with a timeout generous
    /// enough for a loopback connection but still short enough to keep the
    /// tests fast when nothing answers.
    fn network_scan_for(subnet: &str, port: u16) -> NetworkScan {
        NetworkScan::new(
            port,
            vec![Subnet::parse(subnet).expect("valid subnet")],
            Duration::from_millis(200),
        )
        .expect("the network scan should be valid")
    }

    #[tokio::test]
    async fn execution_emits_usb_printers_before_network_progress() {
        let configuration = temporary_configuration("discover-events", "");
        let prepared = prepare(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Network(explicit_network_scan()),
        )
        .expect("network discovery should prepare")
        .with_prober(no_listeners());
        let mut order = Vec::new();
        // A network-only scope never calls the inventory, so an empty
        // fixture is enough to stand in for it here.
        let mut inventory = TolerantInventory { enumeration: None };

        let response = execute(
            prepared,
            |event| match event {
                DiscoveryEvent::Prepared { .. } => order.push("prepared"),
                DiscoveryEvent::UsbPrinter(_) => order.push("usb"),
                DiscoveryEvent::UsbFailure(_) => order.push("usb-failure"),
                DiscoveryEvent::NetworkPrinter(_) => order.push("network"),
                DiscoveryEvent::NetworkScanProgress { .. } => order.push("progress"),
            },
            &mut inventory,
        )
        .await
        .expect("the scan should finish");

        assert_eq!(order.first().copied(), Some("prepared"));
        assert!(order.contains(&"progress"));
        assert!(response.network_printers.is_empty());
    }

    #[tokio::test]
    async fn streamed_usb_and_network_printers_carry_the_same_data_as_the_final_response() {
        let configuration = temporary_configuration("discover-emitted-printers", "");
        let (_listener, port) = loopback_listener([127, 0, 0, 2]);
        let prepared = prepare(
            Some(configuration.path().to_owned()),
            DiscoveryScope::All(network_scan_for("127.0.0.2/32", port)),
        )
        .expect("combined discovery should prepare");
        let mut inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
                failures: vec![UsbEnumerationFailure {
                    stage: UsbFailureStage::OpenDevice,
                    vendor_id: 0x0416,
                    product_id: 0x5012,
                    reason: "denied".to_owned(),
                    permission_denied: true,
                }],
            }),
        };
        let mut emitted_usb = Vec::new();
        let mut emitted_usb_failures = Vec::new();
        let mut emitted_network = Vec::new();

        let response = execute(
            prepared,
            |event| match event {
                DiscoveryEvent::UsbPrinter(printer) => emitted_usb.push(printer.clone()),
                DiscoveryEvent::UsbFailure(failure) => emitted_usb_failures.push(failure.clone()),
                DiscoveryEvent::NetworkPrinter(printer) => emitted_network.push(printer.clone()),
                DiscoveryEvent::Prepared { .. } | DiscoveryEvent::NetworkScanProgress { .. } => {}
            },
            &mut inventory,
        )
        .await
        .expect("combined discovery should finish");

        // A caller streaming these events (a browser showing results as they
        // arrive) must see exactly what the final `Response` reports —
        // otherwise a live view and a completed one would disagree.
        assert_eq!(emitted_usb, response.usb_printers);
        assert_eq!(emitted_usb_failures, response.usb_failures);
        assert_eq!(emitted_network, response.network_printers);
        assert_eq!(
            emitted_network.len(),
            1,
            "the loopback listener should be found"
        );
    }

    #[tokio::test]
    async fn overlapping_subnets_still_emit_exactly_one_network_printer_event() {
        // Two --subnet values that both cover 127.0.0.2 (mirroring
        // `printers discover --subnet 10.0.0.0/25 --subnet 10.0.0.0/24`):
        // 127.0.0.2/32 names it directly, and 127.0.0.0/30 also covers it
        // (its other host address, 127.0.0.1, is this machine's own and
        // gets self-excluded). A streaming consumer must not see the same
        // printer twice just because two targets both happened to probe it.
        let configuration = temporary_configuration("discover-overlap", "");
        let (_listener, port) = loopback_listener([127, 0, 0, 2]);
        let scan = NetworkScan::new(
            port,
            vec![
                Subnet::parse("127.0.0.2/32").expect("valid subnet"),
                Subnet::parse("127.0.0.0/30").expect("valid subnet"),
            ],
            Duration::from_millis(200),
        )
        .expect("the overlapping network scan should be valid");
        let prepared = prepare(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Network(scan),
        )
        .expect("network discovery should prepare");
        let mut inventory = TolerantInventory { enumeration: None };
        let mut emitted_network = Vec::new();

        let response = execute(
            prepared,
            |event| {
                if let DiscoveryEvent::NetworkPrinter(printer) = event {
                    emitted_network.push(printer.clone());
                }
            },
            &mut inventory,
        )
        .await
        .expect("overlapping discovery should finish");

        assert_eq!(
            emitted_network.len(),
            1,
            "one host answering through two overlapping targets must still fire one event: \
             {emitted_network:?}"
        );
        assert_eq!(
            emitted_network, response.network_printers,
            "the streamed set must equal the final Response's set"
        );
    }

    #[test]
    fn a_prepared_discovery_carries_its_skipped_adapters() {
        let configuration = temporary_configuration("discover-skipped", "");
        let prepared = prepare(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Network(explicit_network_scan()),
        )
        .expect("network discovery should prepare");

        // An explicit subnet resolves without interface detection, so nothing is
        // skipped, and the accessor exists for the automatic case.
        assert!(prepared.skipped().is_empty());
    }

    fn office_network_interface() -> InterfaceAddress {
        // A single /16 adapter and nothing else: the motivating case for
        // Finding 1/2 — every candidate interface is too large to sweep
        // automatically, so automatic detection has nothing left to offer.
        InterfaceAddress {
            name: "enp5s0".to_owned(),
            address: Ipv4Addr::new(10, 0, 0, 5),
            netmask: Ipv4Addr::new(255, 255, 0, 0),
        }
    }

    #[test]
    fn resolving_automatic_targets_reports_every_skipped_adapter_without_erroring() {
        // `resolve_targets` itself always calls the real OS for its address
        // list, which a unit test cannot pin down; `resolve_targets_from`
        // is the pure half that does the actual filtering, exercised here
        // with a chosen interface list instead of this machine's real one.
        let (targets, skipped) = resolve_targets_from(&[], vec![office_network_interface()])
            .expect("automatic detection never fails on the subnet limit");

        assert!(targets.is_empty());
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "enp5s0");
        assert_eq!(skipped[0].reason, SkipReason::TooLarge);
    }

    #[test]
    fn an_empty_automatic_sweep_is_not_fatal_for_a_scope_that_still_has_usb() {
        let scope = DiscoveryScope::All(explicit_network_scan());

        assert!(empty_targets_error(&scope, &[]).is_none());
    }

    #[test]
    fn an_empty_automatic_sweep_fails_a_network_only_scope_and_names_the_skipped_adapter() {
        let scope = DiscoveryScope::Network(explicit_network_scan());
        let skipped = vec![SkippedInterface {
            name: "enp5s0".to_owned(),
            subnet: Some(Subnet::parse("10.0.0.0/16").expect("valid subnet")),
            reason: SkipReason::TooLarge,
        }];

        let error = empty_targets_error(&scope, &skipped)
            .expect("a network-only scope with nothing to scan must fail");

        assert!(matches!(error, ApplicationError::NoDiscoverableSubnets(_)));
        assert_eq!(
            error.to_string(),
            "no directly connected IPv4 network is small enough to scan automatically \
             (at most /24): enp5s0 (10.0.0.0/16): larger than /24"
        );
    }

    #[test]
    fn an_empty_automatic_sweep_with_nothing_skipped_still_names_no_adapters() {
        // No non-loopback interface existed at all, so there is nothing to
        // list — the message falls back to its unqualified form rather than
        // a dangling ": ".
        let scope = DiscoveryScope::Network(explicit_network_scan());

        let error = empty_targets_error(&scope, &[])
            .expect("a network-only scope with nothing to scan must fail");

        assert_eq!(
            error.to_string(),
            "no directly connected IPv4 network is small enough to scan automatically (at most /24)"
        );
    }

    #[tokio::test]
    async fn usb_scope_enumerates_usb_without_accepting_network_results() {
        let configuration = temporary_configuration("discover-usb-scope", "");
        let mut inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
                failures: Vec::new(),
            }),
        };

        let response = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Usb,
            &mut inventory,
        )
        .await
        .expect("USB discovery should build its response");

        assert_eq!(response.usb_printers.len(), 1);
        assert!(response.network_printers.is_empty());
        assert!(response.scan_targets.is_empty());
    }

    #[tokio::test]
    async fn network_scope_scans_network_without_enumerating_usb() {
        let configuration = temporary_configuration("discover-network-scope", "");
        let (_listener, port) = loopback_listener([127, 0, 0, 2]);
        let mut inventory = TolerantInventory { enumeration: None };

        let response = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Network(network_scan_for("127.0.0.2/32", port)),
            &mut inventory,
        )
        .await
        .expect("network discovery should build its response");

        assert!(response.usb_printers.is_empty());
        assert_eq!(response.network_printers.len(), 1);
        assert_eq!(response.scan_targets.len(), 1);
    }

    #[tokio::test]
    async fn all_scope_combines_usb_and_network_results() {
        let configuration = temporary_configuration("discover-all-scope", "");
        let (_listener, port) = loopback_listener([127, 0, 0, 2]);
        let mut inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
                failures: Vec::new(),
            }),
        };

        let response = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::All(network_scan_for("127.0.0.2/32", port)),
            &mut inventory,
        )
        .await
        .expect("combined discovery should build its response");

        assert_eq!(response.usb_printers.len(), 1);
        assert_eq!(response.network_printers.len(), 1);
        assert_eq!(response.scan_targets.len(), 1);
    }

    #[tokio::test]
    async fn discover_returns_structured_results_and_tolerant_usb_failure_facts_without_output() {
        let configuration = temporary_configuration("typed-discover", "");
        let mut inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: Vec::new(),
                failures: vec![UsbEnumerationFailure {
                    stage: UsbFailureStage::OpenDevice,
                    vendor_id: 0x0416,
                    product_id: 0x5011,
                    reason: "denied".to_owned(),
                    permission_denied: true,
                }],
            }),
        };
        let (_listener, port) = loopback_listener([127, 0, 0, 2]);
        let targets = vec![ScanTarget {
            subnet: Subnet::parse("127.0.0.2/32").expect("valid subnet"),
            interface: None,
            excluded: Vec::new(),
        }];

        let response = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::All(network_scan_for("127.0.0.2/32", port)),
            &mut inventory,
        )
        .await
        .expect("partial USB failure should not abort discovery");

        assert_eq!(response.scan_targets, targets);
        assert_eq!(response.usb_failures.len(), 1);
        assert_eq!(response.usb_failures[0].stage, UsbFailureStage::OpenDevice);
        assert_eq!(response.usb_failures[0].vendor_id, 0x0416);
        assert_eq!(response.usb_failures[0].product_id, 0x5011);
        assert_eq!(response.usb_failures[0].reason, "denied");
        assert!(response.usb_failures[0].permission_denied);
        assert_eq!(response.network_printers.len(), 1);
        assert_eq!(response.network_printers[0].host, "127.0.0.2");
        assert!(response.registration.network);
    }

    #[tokio::test]
    async fn discover_transforms_configured_and_new_usb_and_network_results() {
        // Two loopback addresses sharing one port: `discovery::scan` probes
        // every target host on a single shared port, so two distinct
        // "printers" need two addresses rather than two ports. 127.0.0.1 is
        // this machine's own address and would be self-excluded, hence
        // starting at .2 (see `loopback_listener`).
        let (_configured_listener, port) = loopback_listener([127, 0, 0, 2]);
        let _unconfigured_listener = TcpListener::bind((Ipv4Addr::new(127, 0, 0, 3), port))
            .expect("the same port should also bind on a second loopback address");
        let configuration = temporary_configuration(
            "discover-transformation",
            &format!(
                r#"
[counter]
transport = "usb"
vendor_id = "0x0416"
product_id = "0x5011"
serial_number = "B120300001"
interface_number = 0
out_endpoint = "0x01"
profile = "NT-5890K"

[kitchen]
transport = "network"
host = "127.0.0.2"
port = {port}
profile = "TM-T88V"

[kitchen-alias]
transport = "network"
host = "127.0.0.2"
port = {port}
"#
            ),
        );
        let configured_usb = netum_usb_printer(vec![0x01], vec![0x81]);
        let mut new_usb = netum_usb_printer(vec![0x02], vec![0x82]);
        new_usb.vendor_id = 0x1234;
        new_usb.product_id = 0xabcd;
        new_usb.bus = "001".to_owned();
        new_usb.address = 7;
        new_usb.serial_number = None;
        let mut inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: vec![configured_usb, new_usb],
                failures: Vec::new(),
            }),
        };

        let response = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::All(network_scan_for("127.0.0.2/31", port)),
            &mut inventory,
        )
        .await
        .expect("the typed discovery response should be built");

        assert_eq!(response.usb_printers.len(), 2);
        let configured_usb = response
            .usb_printers
            .iter()
            .find(|printer| printer.configured_name.as_deref() == Some("counter"))
            .expect("the configured USB printer should be classified");
        assert_eq!(
            configured_usb.configured_profile.as_deref(),
            Some("NT-5890K")
        );
        assert!(
            response
                .usb_printers
                .iter()
                .any(|printer| printer.configured_name.is_none())
        );
        assert_eq!(response.network_printers.len(), 2);
        assert_eq!(
            response.network_printers[0].configured_names,
            vec!["kitchen".to_owned(), "kitchen-alias".to_owned()]
        );
        assert_eq!(
            response.network_printers[0].configured_profile.as_deref(),
            Some("TM-T88V")
        );
        assert!(response.network_printers[1].configured_names.is_empty());
        assert!(response.registration.usb);
        assert!(response.registration.network);
    }

    #[tokio::test]
    async fn discover_transport_filters_skip_the_other_transport() {
        let configuration = temporary_configuration("discover-transport-filter", "");
        let (_listener, port) = loopback_listener([127, 0, 0, 2]);
        let mut network_inventory = TolerantInventory { enumeration: None };

        let network = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Network(network_scan_for("127.0.0.2/32", port)),
            &mut network_inventory,
        )
        .await
        .expect("network-only discovery should not enumerate USB");
        assert!(network.usb_printers.is_empty());
        assert_eq!(network.network_printers.len(), 1);

        let mut usb_inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
                failures: Vec::new(),
            }),
        };
        let usb = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Usb,
            &mut usb_inventory,
        )
        .await
        .expect("USB-only discovery should ignore network hosts");
        assert_eq!(usb.usb_printers.len(), 1);
        assert!(usb.network_printers.is_empty());
    }
}
