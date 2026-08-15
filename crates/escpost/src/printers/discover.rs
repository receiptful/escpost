//! The `printers discover` command: sweep USB and the local network for
//! printers, print what was found, and hint at how to register anything new.

use std::io::{self, Write};
use std::time::Duration;

use crate::cli::{DiscoverPrintersArgs, InventoryTransport};
use crate::configuration::{ConfiguredNetworkPrinter, PrinterConfiguration};
use crate::discovery::{self, DiscoveredHost, ScanTarget, Subnet};
use crate::error::CliError;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use super::inventory::{
    ConnectedUsbPrinter, NusbInventory, UsbInventory, UsbPrinter, classify_usb_printers,
    sort_by_usb_location,
};
use super::output::{
    NetworkListing, UsbListing, format_network_endpoint, usb_printer_label_parts,
    write_network_listing, write_usb_listing,
};

/// What `scan_with_progress` prints before the sweep starts: a
/// `Scanning <N> network(s) on port <port>:` header (singular only for
/// exactly one target), followed by one indented `  - <CIDR>` line per
/// target, with the interface name in parentheses when known
/// (auto-detected targets carry one; explicit `--subnet` targets do not).
/// No trailing newline — `eprintln!` supplies the final one.
fn scan_announcement(targets: &[ScanTarget], port: u16) -> String {
    let count = targets.len();
    let noun = if count == 1 { "network" } else { "networks" };
    let mut announcement = format!("Scanning {count} {noun} on port {port}:");
    for target in targets {
        announcement.push_str("\n  - ");
        announcement.push_str(&target.subnet.to_string());
        if let Some(interface) = &target.interface {
            announcement.push_str(&format!(" ({interface})"));
        }
    }
    announcement
}
/// Run the network sweep behind a progress bar on stderr, shared by `printers
/// discover`'s network portion and the `add --discover` path so the two
/// don't grow diverging bar setups. Hidden automatically by indicatif when
/// stderr is not a terminal (piped output stays byte-identical), so there is
/// no tty check here. Before the bar starts, `scan_announcement` lists every
/// swept network on stderr; `auto_detected` additionally prints a tip toward
/// `--subnet` when the targets came from automatic detection rather than an
/// explicit flag, since only then is there something to override.
pub(super) async fn scan_with_progress(
    targets: &[ScanTarget],
    port: u16,
    probe_timeout: Duration,
    auto_detected: bool,
) -> Vec<DiscoveredHost> {
    eprintln!("{}", scan_announcement(targets, port));
    if auto_detected {
        eprintln!("Tip: pass --subnet <CIDR> to scan a different network.");
    }
    let bar = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::stderr());
    bar.set_style(
        ProgressStyle::with_template("{msg} [{bar:40}] {pos}/{len}")
            .expect("the progress bar template is a compile-time constant")
            .progress_chars("=> "),
    );
    bar.set_message("Scanning for network printers");
    let mut length_set = false;
    let hosts = discovery::scan(targets, port, probe_timeout, |done, total| {
        if !length_set {
            bar.set_length(total);
            length_set = true;
        }
        bar.set_position(done);
    })
    .await;
    // Always clear before any listing/warning/hint output, so the bar never
    // lingers in or interleaves with real output.
    bar.finish_and_clear();
    hosts
}
pub(super) async fn run_discover(
    arguments: DiscoverPrintersArgs,
    configuration: &PrinterConfiguration,
) -> Result<(), CliError> {
    if arguments.transport == Some(InventoryTransport::Usb)
        && (!arguments.subnet.is_empty() || arguments.port.is_some() || arguments.timeout.is_some())
    {
        return Err(CliError::NetworkScanOptionForUsbDiscovery);
    }
    let port = arguments.port.unwrap_or(9100);
    if port == 0 {
        return Err(CliError::InvalidPrinterPort);
    }
    let hosts = if arguments.transport == Some(InventoryTransport::Usb) {
        Vec::new()
    } else {
        let targets = discovery_targets(&arguments.subnet)?;
        scan_with_progress(
            &targets,
            port,
            Duration::from_millis(arguments.timeout.unwrap_or(1000)),
            arguments.subnet.is_empty(),
        )
        .await
    };
    let mut inventory = NusbInventory;
    let connected = execute_discover(
        &mut inventory,
        configuration,
        &hosts,
        arguments.transport,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )?;
    let new_usb = any_new_usb_printer(&connected);
    let new_network = any_new_network_host(&hosts, configuration);
    if let Some(hint) = combined_registration_hint(new_usb, new_network, port) {
        eprintln!("{hint}");
    }
    Ok(())
}
/// The pure core of `printers discover`: enumerate USB (unless
/// `--transport network`) and format the sweep hosts (unless `--transport
/// usb`), printing USB blocks before network blocks with continuous
/// numbering. USB enumeration is best-effort (see `UsbInventory::
/// list_tolerant`): a device that could not be opened or inspected is
/// reported as a warning on `warnings_output` before anything is written to
/// `output`, and the rest of the sweep still runs. Returns the connected USB
/// printers so the caller can also build the USB registration hint without
/// enumerating USB devices twice.
fn execute_discover(
    inventory: &mut impl UsbInventory,
    configuration: &PrinterConfiguration,
    hosts: &[DiscoveredHost],
    transport: Option<InventoryTransport>,
    output: &mut impl Write,
    warnings_output: &mut impl Write,
) -> Result<Vec<ConnectedUsbPrinter>, CliError> {
    let connected = if transport == Some(InventoryTransport::Network) {
        Vec::new()
    } else {
        let usb_enumeration = inventory.list_tolerant()?;
        for warning in &usb_enumeration.warnings {
            writeln!(warnings_output, "Warning: {warning}").map_err(CliError::WriteHumanOutput)?;
        }
        #[cfg(target_os = "linux")]
        if usb_enumeration.permission_denied {
            writeln!(
                warnings_output,
                "Fix USB permissions with: sudo escpost printers grant-usb-permissions"
            )
            .map_err(CliError::WriteHumanOutput)?;
        }
        discovered_usb_printers(usb_enumeration.printers, configuration)
    };
    let hosts: &[DiscoveredHost] = if transport == Some(InventoryTransport::Usb) {
        &[]
    } else {
        hosts
    };

    if connected.is_empty() && hosts.is_empty() {
        writeln!(output, "No printers discovered.").map_err(CliError::WriteHumanOutput)?;
        return Ok(connected);
    }

    write_discovered_usb_printers(output, &connected, configuration)?;
    write_discovered_network_printers(output, hosts, configuration, connected.len() + 1)?;
    Ok(connected)
}
/// Explicit --subnet values are scanned exactly as given; without them the
/// sweep covers every small directly connected network.
pub(super) fn discovery_targets(subnets: &[Subnet]) -> Result<Vec<ScanTarget>, CliError> {
    if subnets.is_empty() {
        let targets = discovery::local_scan_targets()?;
        if targets.is_empty() {
            return Err(CliError::NoDiscoverableSubnets);
        }
        return Ok(targets);
    }
    Ok(subnets
        .iter()
        .map(|subnet| ScanTarget {
            subnet: *subnet,
            interface: None,
            excluded: None,
        })
        .collect())
}
/// Write each connected USB printer's block, numbered from 1. New printers
/// (no matching configuration) head their block with the product string
/// alone (falling back to a generic label, like `printers list`'s own
/// missing-product handling), `status: new`, and no `model:`/`profile:`
/// lines; configured printers head it with the saved name, `status:
/// configured`, and both lines, falling back to `unassigned` like `printers
/// list`. Either way, `write_usb_listing` adds a `manufacturer:` line
/// whenever the device reports one, regardless of `status`.
fn write_discovered_usb_printers(
    output: &mut impl Write,
    connected: &[ConnectedUsbPrinter],
    configuration: &PrinterConfiguration,
) -> Result<(), CliError> {
    for (offset, connected) in connected.iter().enumerate() {
        let configured = connected
            .configuration_index
            .map(|index| &configuration.usb_printers()[index]);
        // `None` here drops the manufacturer suffix `usb_printer_label_parts`
        // would otherwise append: the manufacturer is its own line now (see
        // `write_usb_listing`), not folded into the product string.
        let product = usb_printer_label_parts(connected.printer.product.as_deref(), None);
        let listing = match configured {
            Some(configured) => UsbListing {
                heading: &configured.name,
                status: "configured",
                model: Some(product.as_str()),
                profile: Some(configured.profile.as_deref()),
                printer: &connected.printer,
            },
            None => UsbListing {
                heading: &product,
                status: "new",
                model: None,
                profile: None,
                printer: &connected.printer,
            },
        };
        write_usb_listing(output, offset + 1, &listing)?;
    }
    Ok(())
}
/// Write each discovered network host's block, numbered starting at `start`
/// so USB blocks (numbered 1..) can precede it in the combined listing.
fn write_discovered_network_printers(
    output: &mut impl Write,
    hosts: &[DiscoveredHost],
    configuration: &PrinterConfiguration,
    start: usize,
) -> Result<(), CliError> {
    for (offset, host) in hosts.iter().enumerate() {
        let address = host.address.to_string();
        let endpoint = format_network_endpoint(&address, host.port);
        let matches = configured_network_printers(configuration, host);
        let also_configured: Vec<&str> = matches
            .iter()
            .skip(1)
            .map(|printer| printer.name.as_str())
            .collect();
        let listing = if let Some(first) = matches.first() {
            NetworkListing {
                heading: &first.name,
                status: "configured",
                profile: Some(first.profile.as_deref()),
                host: &address,
                port: host.port,
                interface: host.interface.as_deref(),
                also_configured: &also_configured,
            }
        } else {
            NetworkListing {
                heading: &endpoint,
                status: "new",
                profile: None,
                host: &address,
                port: host.port,
                interface: host.interface.as_deref(),
                also_configured: &[],
            }
        };
        write_network_listing(output, start + offset, &listing)?;
    }
    Ok(())
}
/// Saved network printers matching a discovered endpoint, in configuration
/// order. Matching is textual on host and exact on port; saved hostnames
/// never match.
fn configured_network_printers<'a>(
    configuration: &'a PrinterConfiguration,
    host: &DiscoveredHost,
) -> Vec<&'a ConfiguredNetworkPrinter> {
    configuration
        .network_printers()
        .iter()
        .filter(|printer| printer.port == host.port && printer.host == host.address.to_string())
        .collect()
}
/// Names of saved network printers matching a discovered endpoint.
pub(super) fn configured_names<'a>(
    configuration: &'a PrinterConfiguration,
    host: &DiscoveredHost,
) -> Vec<&'a str> {
    configured_network_printers(configuration, host)
        .into_iter()
        .map(|printer| printer.name.as_str())
        .collect()
}
/// Whether at least one discovered network host does not match a saved
/// printer's host and port. Count-independent by design: `combined_
/// registration_hint` only cares whether the network transport found
/// anything new, not how much.
fn any_new_network_host(hosts: &[DiscoveredHost], configuration: &PrinterConfiguration) -> bool {
    hosts
        .iter()
        .any(|host| configured_names(configuration, host).is_empty())
}
/// Whether at least one connected USB printer does not yet match a saved
/// identity. Count-independent for the same reason as `any_new_network_host`.
fn any_new_usb_printer(connected: &[ConnectedUsbPrinter]) -> bool {
    connected
        .iter()
        .any(|printer| printer.configuration_index.is_none())
}
/// The single registration hint `printers discover` prints to stderr after
/// the listing, chosen by which transports found at least one new
/// (unconfigured) printer — count-independent within each transport, so a
/// caller passes `any_new_usb_printer`/`any_new_network_host`'s bool, not a
/// count. `None` when neither transport found anything new, including an
/// empty sweep. USB-only and network-only each point at that transport's
/// non-interactive `add` invocation (the network form gets a `--port`
/// suffix when `port` is not the default 9100). Finding new printers on
/// *both* transports instead points at the bare interactive `add` wizard —
/// it prompts for the transport itself, so one command still covers the
/// case rather than printing two hint lines or guessing which transport the
/// user meant.
fn combined_registration_hint(new_usb: bool, new_network: bool, port: u16) -> Option<String> {
    match (new_usb, new_network) {
        (false, false) => None,
        (true, false) => Some(
            "Register a new USB printer with: escpost printers add <NAME> --transport usb"
                .to_owned(),
        ),
        (false, true) => {
            let port_suffix = if port == 9100 {
                String::new()
            } else {
                format!(" --port {port}")
            };
            Some(format!(
                "Register a new network printer with: escpost printers add <NAME> --transport network --discover{port_suffix}"
            ))
        }
        (true, true) => Some("Register a new printer with: escpost printers add <NAME>".to_owned()),
    }
}
/// Classify connected USB printers for `printers discover`. Unlike
/// `merge_usb_identities`, this keeps the stable-location order instead of
/// re-sorting by display name afterward: `list` groups by name, but
/// `discover`'s USB block simply reports every connected printer as it is
/// found, the same way its network sweep already reports hosts in scan
/// order. Printers not claimed by any configuration are left `None`, ready
/// to be reported as newly discovered.
fn discovered_usb_printers(
    mut printers: Vec<UsbPrinter>,
    configuration: &PrinterConfiguration,
) -> Vec<ConnectedUsbPrinter> {
    sort_by_usb_location(&mut printers);
    classify_usb_printers(printers, configuration).0
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::super::inventory::{UsbDeviceIdentity, UsbEnumeration, UsbPrinter};
    use super::super::test_support::{
        FixedInventory, discovered, netum_usb_printer, usb_printer_identity,
    };
    use super::*;
    use crate::configuration::PrinterConfiguration;

    /// A `UsbInventory` double that exercises `list_tolerant`'s partial-
    /// failure path directly: some devices enumerate fine, others report a
    /// canned warning, mirroring what `NusbInventory::list_tolerant` does
    /// when a real device cannot be opened or inspected. `list()` stays
    /// strict (as `printers list`/`add` need), returning only the printers
    /// that "succeeded".
    struct PartiallyFailingInventory {
        printers: Vec<UsbPrinter>,
        warnings: Vec<String>,
        /// Set directly by the test rather than derived from `warnings`'
        /// text: production computes this the same way, from the
        /// structured `CliError` at the point of failure
        /// (`CliError::is_permission_denied_usb_open`, checked in
        /// `NusbInventory::list_tolerant` before the error is ever
        /// formatted into a warning string), not by pattern-matching the
        /// formatted message afterward.
        permission_denied: bool,
    }

    impl UsbInventory for PartiallyFailingInventory {
        fn list(&mut self) -> Result<Vec<UsbPrinter>, CliError> {
            Ok(self.printers.clone())
        }

        fn list_tolerant(&mut self) -> Result<UsbEnumeration, CliError> {
            Ok(UsbEnumeration {
                printers: self.printers.clone(),
                warnings: self.warnings.clone(),
                permission_denied: self.permission_denied,
            })
        }

        fn identities(&mut self) -> Result<Vec<UsbDeviceIdentity>, CliError> {
            Ok(self.printers.iter().map(usb_printer_identity).collect())
        }
    }

    #[test]
    fn discovered_hosts_print_full_listing_blocks_by_configuration_state() {
        let configuration = PrinterConfiguration::parse(
            r#"
[kitchen]
transport = "network"
host = "10.42.0.71"
port = 9100
profile = "TM-T88V"

[office]
transport = "network"
host = "10.42.0.9"
port = 9100

[counter]
transport = "network"
host = "10.42.0.20"
port = 9100
profile = "EPSON-TM88"

[counter-spare]
transport = "network"
host = "10.42.0.20"
port = 9100
"#,
        )
        .expect("the existing printers should parse");
        let hosts = vec![
            DiscoveredHost {
                address: Ipv4Addr::new(10, 42, 0, 5),
                port: 9100,
                interface: None,
            },
            DiscoveredHost {
                address: Ipv4Addr::new(10, 42, 0, 71),
                port: 9100,
                interface: Some("enx0".to_owned()),
            },
            DiscoveredHost {
                address: Ipv4Addr::new(10, 42, 0, 9),
                port: 9100,
                interface: None,
            },
            DiscoveredHost {
                address: Ipv4Addr::new(10, 42, 0, 20),
                port: 9100,
                interface: None,
            },
        ];
        let mut output = Vec::new();

        write_discovered_network_printers(&mut output, &hosts, &configuration, 1)
            .expect("writing the listing should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] 10.42.0.5:9100
    status: new
    transport: network
    network: 10.42.0.5:9100
[2] kitchen
    status: configured
    profile: TM-T88V
    transport: network
    network: 10.42.0.71:9100
    interface: enx0
[3] office
    status: configured
    profile: unassigned
    transport: network
    network: 10.42.0.9:9100
[4] counter
    status: configured
    profile: EPSON-TM88
    transport: network
    network: 10.42.0.20:9100
    also configured as: counter-spare
"
        );
    }

    #[test]
    fn any_new_network_host_is_true_for_a_newly_discovered_host() {
        let hosts = vec![discovered([10, 42, 0, 71], 9100)];

        assert!(any_new_network_host(
            &hosts,
            &PrinterConfiguration::default()
        ));
    }

    #[test]
    fn any_new_network_host_does_not_depend_on_how_many_new_hosts_were_found() {
        let one_new_host = vec![discovered([10, 42, 0, 71], 9100)];
        let several_new_hosts = vec![
            discovered([10, 42, 0, 5], 9100),
            discovered([10, 42, 0, 71], 9100),
        ];

        assert!(any_new_network_host(
            &one_new_host,
            &PrinterConfiguration::default()
        ));
        assert!(any_new_network_host(
            &several_new_hosts,
            &PrinterConfiguration::default()
        ));
    }

    #[test]
    fn any_new_network_host_is_false_when_every_discovered_host_is_already_configured() {
        let configuration = PrinterConfiguration::parse(
            r#"
[kitchen]
transport = "network"
host = "10.42.0.71"
port = 9100
"#,
        )
        .expect("the existing printer should parse");
        let hosts = vec![discovered([10, 42, 0, 71], 9100)];

        assert!(!any_new_network_host(&hosts, &configuration));
    }

    #[test]
    fn any_new_network_host_is_false_for_an_empty_sweep() {
        assert!(!any_new_network_host(&[], &PrinterConfiguration::default()));
    }

    #[test]
    fn combined_registration_hint_is_none_when_neither_transport_found_a_new_printer() {
        assert_eq!(combined_registration_hint(false, false, 9100), None);
    }

    #[test]
    fn combined_registration_hint_for_a_new_usb_printer_only() {
        let hint = combined_registration_hint(true, false, 9100);

        assert_eq!(
            hint,
            Some(
                "Register a new USB printer with: escpost printers add <NAME> --transport usb"
                    .to_owned()
            )
        );
    }

    #[test]
    fn combined_registration_hint_for_a_new_network_host_only_at_the_default_port() {
        let hint = combined_registration_hint(false, true, 9100);

        assert_eq!(
        hint,
        Some(
            "Register a new network printer with: escpost printers add <NAME> --transport network --discover"
                .to_owned()
        )
    );
    }

    #[test]
    fn combined_registration_hint_for_a_new_network_host_only_at_a_non_default_port() {
        let hint = combined_registration_hint(false, true, 9200);

        assert_eq!(
        hint,
        Some(
            "Register a new network printer with: escpost printers add <NAME> --transport network --discover --port 9200"
                .to_owned()
        )
    );
    }

    #[test]
    fn combined_registration_hint_for_new_printers_on_both_transports() {
        let hint = combined_registration_hint(true, true, 9100);

        assert_eq!(
            hint,
            Some("Register a new printer with: escpost printers add <NAME>".to_owned())
        );
    }

    #[test]
    fn combined_registration_hint_for_new_printers_on_both_transports_ignores_the_network_port() {
        // The interactive wizard prompts for the transport (and, if network,
        // the port), so the "both" hint never grows a `--port` suffix even
        // when the scan used a non-default port.
        let hint = combined_registration_hint(true, true, 9200);

        assert_eq!(
            hint,
            Some("Register a new printer with: escpost printers add <NAME>".to_owned())
        );
    }

    #[test]
    fn scan_announcement_names_a_single_auto_detected_target_with_its_interface() {
        let targets = vec![ScanTarget {
            subnet: Subnet::parse("10.42.0.0/24").expect("a valid CIDR should parse"),
            interface: Some("enx0".to_owned()),
            excluded: None,
        }];

        let announcement = scan_announcement(&targets, 9100);

        assert_eq!(
            announcement,
            "Scanning 1 network on port 9100:\n  - 10.42.0.0/24 (enx0)"
        );
    }

    #[test]
    fn scan_announcement_uses_singular_network_for_a_single_explicit_target() {
        let targets = vec![ScanTarget {
            subnet: Subnet::parse("127.0.0.1/32").expect("a valid CIDR should parse"),
            interface: None,
            excluded: None,
        }];

        let announcement = scan_announcement(&targets, 9100);

        assert_eq!(
            announcement,
            "Scanning 1 network on port 9100:\n  - 127.0.0.1/32"
        );
    }

    #[test]
    fn scan_announcement_mixes_auto_detected_and_explicit_targets() {
        let targets = vec![
            ScanTarget {
                subnet: Subnet::parse("10.42.0.0/24").expect("a valid CIDR should parse"),
                interface: Some("enx0".to_owned()),
                excluded: None,
            },
            ScanTarget {
                subnet: Subnet::parse("127.0.0.1/32").expect("a valid CIDR should parse"),
                interface: None,
                excluded: None,
            },
        ];

        let announcement = scan_announcement(&targets, 9200);

        assert_eq!(
            announcement,
            "Scanning 2 networks on port 9200:\n  - 10.42.0.0/24 (enx0)\n  - 127.0.0.1/32"
        );
    }

    #[test]
    fn scan_announcement_lists_explicit_targets_without_an_interface() {
        let targets = vec![
            ScanTarget {
                subnet: Subnet::parse("10.42.0.0/24").expect("a valid CIDR should parse"),
                interface: None,
                excluded: None,
            },
            ScanTarget {
                subnet: Subnet::parse("192.168.0.0/24").expect("a valid CIDR should parse"),
                interface: None,
                excluded: None,
            },
        ];

        let announcement = scan_announcement(&targets, 9100);

        assert_eq!(
            announcement,
            "Scanning 2 networks on port 9100:\n  - 10.42.0.0/24\n  - 192.168.0.0/24"
        );
    }

    #[test]
    fn discover_reports_a_new_usb_printer_with_no_model_or_profile_line() {
        let mut inventory = FixedInventory {
            printers: vec![UsbPrinter {
                vendor_id: 0x0416,
                product_id: 0x5011,
                bus: "3".to_owned(),
                address: 57,
                manufacturer: Some("YICHIP3121".to_owned()),
                product: Some("USB Portable Printer".to_owned()),
                serial_number: Some("B120300001".to_owned()),
                interface_number: 0,
                out_endpoints: vec![0x01],
                in_endpoints: vec![0x81],
            }],
        };
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &PrinterConfiguration::default(),
            &[],
            None,
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        // The heading is the product string alone (no more combined "product
        // (manufacturer)" label), but the manufacturer still shows up as its
        // own line in the block below, right where `model:` would be if this
        // device were configured.
        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] USB Portable Printer
    status: new
    manufacturer: YICHIP3121
    transport: usb
    usb: 0416:5011; bus 3 address 57; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300001
"
        );
    }

    #[test]
    fn discover_falls_back_to_a_generic_heading_when_the_new_device_has_no_product_string() {
        let mut inventory = FixedInventory {
            printers: vec![UsbPrinter {
                vendor_id: 0x0416,
                product_id: 0x5011,
                bus: "3".to_owned(),
                address: 57,
                manufacturer: None,
                product: None,
                serial_number: None,
                interface_number: 0,
                out_endpoints: vec![0x01],
                in_endpoints: Vec::new(),
            }],
        };
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &PrinterConfiguration::default(),
            &[],
            None,
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert!(
            output.starts_with("[1] USB printer\n"),
            "a device with no product string at all must fall back to a generic heading:\n{output}"
        );
        assert!(!output.contains("manufacturer:"));
    }

    #[test]
    fn discover_reports_a_configured_usb_printer_with_model_and_profile_lines() {
        let mut inventory = FixedInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[netum-usb]
transport = \"usb\"
profile = \"NT-5890K\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
serial_number = \"B120300001\"
interface_number = 0
out_endpoint = \"0x01\"
in_endpoint = \"0x81\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &configuration,
            &[],
            None,
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] netum-usb
    status: configured
    model: USB Portable Printer
    manufacturer: YICHIP3121
    profile: NT-5890K
    transport: usb
    usb: 0416:5011; bus 003 address 60; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300001
"
        );
    }

    #[test]
    fn discover_omits_the_manufacturer_line_when_the_device_reports_none() {
        let mut printer = netum_usb_printer(vec![0x01], vec![0x81]);
        printer.manufacturer = None;
        let mut inventory = FixedInventory {
            printers: vec![printer],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[netum-usb]
transport = \"usb\"
profile = \"NT-5890K\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
serial_number = \"B120300001\"
interface_number = 0
out_endpoint = \"0x01\"
in_endpoint = \"0x81\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &configuration,
            &[],
            None,
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] netum-usb
    status: configured
    model: USB Portable Printer
    profile: NT-5890K
    transport: usb
    usb: 0416:5011; bus 003 address 60; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300001
"
        );
    }

    #[test]
    fn discover_configured_usb_printer_can_remain_unprofiled() {
        let mut inventory = FixedInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[uncalibrated-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("an unprofiled USB printer should be valid");
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &configuration,
            &[],
            None,
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        assert!(
            String::from_utf8(output)
                .expect("the listing should be UTF-8")
                .contains("profile: unassigned")
        );
    }

    #[test]
    fn discover_one_saved_identity_names_at_most_one_connected_interface() {
        let mut inventory = FixedInventory {
            printers: vec![
                UsbPrinter {
                    vendor_id: 0x1000,
                    product_id: 0x0001,
                    bus: "2".to_owned(),
                    address: 2,
                    manufacturer: None,
                    product: Some("Second Model".to_owned()),
                    serial_number: None,
                    interface_number: 0,
                    out_endpoints: vec![0x01],
                    in_endpoints: Vec::new(),
                },
                UsbPrinter {
                    vendor_id: 0x1000,
                    product_id: 0x0001,
                    bus: "1".to_owned(),
                    address: 1,
                    manufacturer: None,
                    product: Some("First Model".to_owned()),
                    serial_number: None,
                    interface_number: 0,
                    out_endpoints: vec![0x01],
                    in_endpoints: Vec::new(),
                },
            ],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[shared-identity]
transport = \"usb\"
profile = \"GENERIC\"
vendor_id = \"0x1000\"
product_id = \"0x0001\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &configuration,
            &[],
            None,
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert_eq!(output.matches("] shared-identity\n").count(), 1);
        assert_eq!(output.matches("status: configured").count(), 1);
        assert_eq!(output.matches("status: new").count(), 1);
    }

    #[test]
    fn discover_numbers_usb_blocks_before_network_blocks_continuously() {
        let mut inventory = FixedInventory {
            printers: vec![
                UsbPrinter {
                    vendor_id: 0x0416,
                    product_id: 0x5011,
                    bus: "3".to_owned(),
                    address: 57,
                    manufacturer: Some("YICHIP3121".to_owned()),
                    product: Some("USB Portable Printer".to_owned()),
                    serial_number: Some("B120300001".to_owned()),
                    interface_number: 0,
                    out_endpoints: vec![0x01],
                    in_endpoints: vec![0x81],
                },
                UsbPrinter {
                    vendor_id: 0x0416,
                    product_id: 0x5011,
                    bus: "3".to_owned(),
                    address: 60,
                    manufacturer: Some("YICHIP3121".to_owned()),
                    product: Some("USB Portable Printer".to_owned()),
                    serial_number: Some("B120300002".to_owned()),
                    interface_number: 0,
                    out_endpoints: vec![0x01],
                    in_endpoints: vec![0x81],
                },
            ],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[netum-usb]
transport = \"usb\"
profile = \"NT-5890K\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
serial_number = \"B120300002\"
interface_number = 0
out_endpoint = \"0x01\"
in_endpoint = \"0x81\"
",
        )
        .expect("the printer configuration should be valid");
        let hosts = vec![discovered([10, 42, 0, 5], 9100)];
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &configuration,
            &hosts,
            None,
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        let headings = String::from_utf8(output)
            .expect("the listing should be UTF-8")
            .lines()
            .filter(|line| line.starts_with('['))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(
            headings,
            vec![
                "[1] USB Portable Printer".to_owned(),
                "[2] netum-usb".to_owned(),
                "[3] 10.42.0.5:9100".to_owned(),
            ]
        );
    }

    #[test]
    fn discover_transport_usb_skips_the_network_section() {
        let mut inventory = FixedInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
        };
        let hosts = vec![discovered([10, 42, 0, 5], 9100)];
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &PrinterConfiguration::default(),
            &hosts,
            Some(InventoryTransport::Usb),
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert!(output.contains("transport: usb"));
        assert!(
            !output.contains("transport: network"),
            "--transport usb must not scan or report network hosts:\n{output}"
        );
    }

    #[test]
    fn discover_transport_network_skips_the_usb_section() {
        let mut inventory = FixedInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
        };
        let hosts = vec![discovered([10, 42, 0, 5], 9100)];
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &PrinterConfiguration::default(),
            &hosts,
            Some(InventoryTransport::Network),
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert!(
            !output.contains("transport: usb"),
            "--transport network must not enumerate or report USB printers:\n{output}"
        );
        assert!(output.contains("transport: network"));
    }

    #[test]
    fn discover_reports_an_empty_combined_result() {
        let mut inventory = FixedInventory {
            printers: Vec::new(),
        };
        let mut output = Vec::new();

        execute_discover(
            &mut inventory,
            &PrinterConfiguration::default(),
            &[],
            None,
            &mut output,
            &mut Vec::new(),
        )
        .expect("discovery should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "No printers discovered.\n"
        );
    }

    #[test]
    fn any_new_usb_printer_is_true_for_a_new_usb_printer() {
        let connected = vec![ConnectedUsbPrinter {
            printer: netum_usb_printer(vec![0x01], vec![0x81]),
            configuration_index: None,
        }];

        assert!(any_new_usb_printer(&connected));
    }

    #[test]
    fn any_new_usb_printer_is_false_when_every_connected_usb_printer_is_configured() {
        let connected = vec![ConnectedUsbPrinter {
            printer: netum_usb_printer(vec![0x01], vec![0x81]),
            configuration_index: Some(0),
        }];

        assert!(!any_new_usb_printer(&connected));
    }

    #[test]
    fn any_new_usb_printer_is_false_for_no_connected_usb_printers() {
        assert!(!any_new_usb_printer(&[]));
    }

    #[test]
    fn both_transports_finding_new_printers_yields_the_combined_hint() {
        let connected = vec![ConnectedUsbPrinter {
            printer: netum_usb_printer(vec![0x01], vec![0x81]),
            configuration_index: None,
        }];
        let hosts = vec![discovered([10, 42, 0, 71], 9100)];

        let new_usb = any_new_usb_printer(&connected);
        let new_network = any_new_network_host(&hosts, &PrinterConfiguration::default());
        let hint = combined_registration_hint(new_usb, new_network, 9100);

        assert_eq!(
            hint,
            Some("Register a new printer with: escpost printers add <NAME>".to_owned())
        );
    }

    #[test]
    fn discover_surfaces_a_per_device_warning_and_still_lists_the_rest() {
        // A non-permission failure shape (config inspection, not open) on
        // purpose: this test is about partial-failure tolerance, not the
        // permission hint below, which has its own dedicated tests.
        let mut inventory = PartiallyFailingInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
            warnings: vec![
                "could not inspect the active configuration of USB device 0416:5012: device is not configured".to_owned(),
            ],
            permission_denied: false,
        };
        let mut output = Vec::new();
        let mut warnings_output = Vec::new();

        execute_discover(
            &mut inventory,
            &PrinterConfiguration::default(),
            &[],
            None,
            &mut output,
            &mut warnings_output,
        )
        .expect("a per-device enumeration failure must not abort discovery");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert!(
            output.contains("[1] USB Portable Printer"),
            "the device that enumerated fine should still be listed:\n{output}"
        );
        let warnings_output =
            String::from_utf8(warnings_output).expect("the warnings should be UTF-8");
        assert_eq!(
            warnings_output,
            "Warning: could not inspect the active configuration of USB device 0416:5012: device is not configured\n"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn discover_appends_the_grant_usb_permissions_hint_once_after_permission_denied_warnings() {
        let mut inventory = PartiallyFailingInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
            warnings: vec![
                "could not open USB device 0416:5012: permission denied (errno 13)".to_owned(),
                "could not open USB device 0416:5013: permission denied (errno 13)".to_owned(),
            ],
            permission_denied: true,
        };
        let mut output = Vec::new();
        let mut warnings_output = Vec::new();

        execute_discover(
            &mut inventory,
            &PrinterConfiguration::default(),
            &[],
            None,
            &mut output,
            &mut warnings_output,
        )
        .expect("a per-device enumeration failure must not abort discovery");

        let warnings_output =
            String::from_utf8(warnings_output).expect("the warnings should be UTF-8");
        assert_eq!(
            warnings_output,
            "\
Warning: could not open USB device 0416:5012: permission denied (errno 13)
Warning: could not open USB device 0416:5013: permission denied (errno 13)
Fix USB permissions with: sudo escpost printers grant-usb-permissions
"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn discover_prints_no_grant_usb_permissions_hint_without_a_permission_denied_warning() {
        let mut inventory = PartiallyFailingInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
            warnings: vec![
                "could not inspect the active configuration of USB device 0416:5012: device is not configured".to_owned(),
            ],
            permission_denied: false,
        };
        let mut output = Vec::new();
        let mut warnings_output = Vec::new();

        execute_discover(
            &mut inventory,
            &PrinterConfiguration::default(),
            &[],
            None,
            &mut output,
            &mut warnings_output,
        )
        .expect("a per-device enumeration failure must not abort discovery");

        let warnings_output =
            String::from_utf8(warnings_output).expect("the warnings should be UTF-8");
        assert!(
            !warnings_output.contains("grant-usb-permissions"),
            "no permission-denied warning means no grant-usb-permissions hint:\n{warnings_output}"
        );
    }
}
