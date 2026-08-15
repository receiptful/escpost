//! The `printers discover` terminal adapter.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use super::super::Transport;
use super::super::cli::output::{
    NetworkListing, UsbListing, format_network_endpoint, usb_printer_label_parts,
    write_network_listing, write_usb_listing,
};
use super::super::cli::{DiscoverPrintersArgs, InventoryTransport};
use super::super::inventory::{UsbEnumerationFailure, UsbFailureStage};
use super::{Request, Response, execute_with_observer};
use crate::application::ApplicationError;
use crate::configuration::{ConfiguredNetworkPrinter, PrinterConfiguration};
use crate::discovery::{self, DiscoveredHost, ScanTarget, Subnet};
use crate::error::CliError;

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

/// Run a network sweep behind the CLI progress display.
pub(crate) async fn scan_with_progress(
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
    bar.finish_and_clear();
    hosts
}

pub(crate) async fn run_discover(
    arguments: DiscoverPrintersArgs,
    config: Option<PathBuf>,
) -> Result<(), CliError> {
    let port = arguments.port.unwrap_or(9100);
    let invalid_usb_options = arguments.transport == Some(InventoryTransport::Usb)
        && (!arguments.subnet.is_empty()
            || arguments.port.is_some()
            || arguments.timeout.is_some());
    if invalid_usb_options {
        return Err(CliError::NetworkScanOptionForUsbDiscovery);
    }
    let bar = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::stderr());
    bar.set_style(
        ProgressStyle::with_template("{msg} [{bar:40}] {pos}/{len}")
            .expect("the progress bar template is a compile-time constant")
            .progress_chars("=> "),
    );
    bar.set_message("Scanning for network printers");
    let mut length_set = false;
    let response = execute_with_observer(
        Request {
            config,
            transport: arguments.transport.map(|transport| match transport {
                InventoryTransport::Usb => Transport::Usb,
                InventoryTransport::Network => Transport::Network,
            }),
            port,
            subnets: arguments.subnet,
            timeout: Duration::from_millis(arguments.timeout.unwrap_or(1000)),
        },
        || Ok(()),
        |path, targets, auto_detected| {
            eprintln!(
                "Reading configuration from {}",
                crate::configuration::display_path(path)
            );
            if !targets.is_empty() {
                eprintln!("{}", scan_announcement(targets, port));
                if auto_detected {
                    eprintln!("Tip: pass --subnet <CIDR> to scan a different network.");
                }
            }
        },
        |done, total| {
            if !length_set {
                bar.set_length(total);
                length_set = true;
            }
            bar.set_position(done);
        },
    )
    .await;
    bar.finish_and_clear();
    let response = response?;
    write_response(
        &response,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )?;
    if let Some(hint) = combined_registration_hint(
        response.registration.usb,
        response.registration.network,
        port,
    ) {
        eprintln!("{hint}");
    }
    Ok(())
}

fn write_response(
    response: &Response,
    output: &mut impl Write,
    warnings_output: &mut impl Write,
) -> Result<(), CliError> {
    for failure in &response.usb_failures {
        write_usb_failure(warnings_output, failure)?;
    }
    #[cfg(target_os = "linux")]
    if response
        .usb_failures
        .iter()
        .any(|failure| failure.permission_denied)
    {
        writeln!(
            warnings_output,
            "Fix USB permissions with: sudo escpost printers grant-usb-permissions"
        )
        .map_err(CliError::WriteHumanOutput)?;
    }
    if response.usb_printers.is_empty() && response.network_printers.is_empty() {
        writeln!(output, "No printers discovered.").map_err(CliError::WriteHumanOutput)?;
        return Ok(());
    }
    for (offset, discovered) in response.usb_printers.iter().enumerate() {
        let product = usb_printer_label_parts(discovered.printer.product.as_deref(), None);
        let listing = match &discovered.configured_name {
            Some(name) => UsbListing {
                heading: name,
                status: "configured",
                model: Some(product.as_str()),
                profile: Some(discovered.configured_profile.as_deref()),
                printer: &discovered.printer,
            },
            None => UsbListing {
                heading: &product,
                status: "new",
                model: None,
                profile: None,
                printer: &discovered.printer,
            },
        };
        write_usb_listing(output, offset + 1, &listing)?;
    }
    let start = response.usb_printers.len() + 1;
    for (offset, discovered) in response.network_printers.iter().enumerate() {
        let endpoint = format_network_endpoint(&discovered.host, discovered.port);
        let also_configured = discovered
            .configured_names
            .iter()
            .skip(1)
            .map(String::as_str)
            .collect::<Vec<_>>();
        let listing = if let Some(first) = discovered.configured_names.first() {
            NetworkListing {
                heading: first,
                status: "configured",
                profile: Some(discovered.configured_profile.as_deref()),
                host: &discovered.host,
                port: discovered.port,
                interface: discovered.interface.as_deref(),
                also_configured: &also_configured,
            }
        } else {
            NetworkListing {
                heading: &endpoint,
                status: "new",
                profile: None,
                host: &discovered.host,
                port: discovered.port,
                interface: discovered.interface.as_deref(),
                also_configured: &[],
            }
        };
        write_network_listing(output, start + offset, &listing)?;
    }
    Ok(())
}

fn write_usb_failure(
    output: &mut impl Write,
    failure: &UsbEnumerationFailure,
) -> Result<(), CliError> {
    let action = match failure.stage {
        UsbFailureStage::OpenDevice => "could not open",
        UsbFailureStage::InspectConfiguration => "could not inspect the active configuration of",
    };
    writeln!(
        output,
        "Warning: {action} USB device {:04x}:{:04x}: {}",
        failure.vendor_id, failure.product_id, failure.reason
    )
    .map_err(CliError::WriteHumanOutput)
}

/// Explicit --subnet values are scanned exactly as given; without them the
/// sweep covers every small directly connected network.
pub(crate) fn discovery_targets(subnets: &[Subnet]) -> Result<Vec<ScanTarget>, CliError> {
    if subnets.is_empty() {
        let targets = discovery::local_scan_targets()?;
        if targets.is_empty() {
            return Err(ApplicationError::NoDiscoverableSubnets.into());
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

/// Saved network printers matching a discovered endpoint, in configuration
/// order. Matching is textual on host and exact on port; saved hostnames never
/// match an address returned by discovery.
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

pub(crate) fn configured_names<'a>(
    configuration: &'a PrinterConfiguration,
    host: &DiscoveredHost,
) -> Vec<&'a str> {
    configured_network_printers(configuration, host)
        .into_iter()
        .map(|printer| printer.name.as_str())
        .collect()
}

/// Choose the single registration hint from the typed response's availability
/// facts. `None` means neither transport found an unconfigured printer.
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

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::path::PathBuf;

    use super::*;
    use crate::features::printers::discover::{
        NetworkDiscovery, RegistrationAvailability, UsbDiscovery,
    };
    use crate::features::printers::inventory::UsbPrinter;

    #[test]
    fn rich_typed_response_uses_the_production_discover_writer() {
        let response = Response {
            config_path: PathBuf::from("/tmp/printers.toml"),
            scan_targets: Vec::new(),
            usb_printers: vec![
                UsbDiscovery {
                    configured_name: None,
                    configured_profile: None,
                    printer: usb_printer("003", 60, Some("B120300001"), Some("YICHIP3121")),
                },
                UsbDiscovery {
                    configured_name: Some("counter".to_owned()),
                    configured_profile: None,
                    printer: usb_printer("004", 61, Some("B120300002"), None),
                },
            ],
            network_printers: vec![
                NetworkDiscovery {
                    configured_names: Vec::new(),
                    configured_profile: None,
                    host: "10.42.0.5".to_owned(),
                    port: 9100,
                    interface: None,
                },
                NetworkDiscovery {
                    configured_names: vec!["kitchen".to_owned(), "kitchen-spare".to_owned()],
                    configured_profile: Some("TM-T88V".to_owned()),
                    host: "2001:db8::5".to_owned(),
                    port: 9100,
                    interface: Some("enx0".to_owned()),
                },
            ],
            usb_failures: vec![UsbEnumerationFailure {
                stage: UsbFailureStage::InspectConfiguration,
                vendor_id: 0x0416,
                product_id: 0x5012,
                reason: "device is not configured".to_owned(),
                permission_denied: false,
            }],
            registration: RegistrationAvailability {
                usb: true,
                network: true,
            },
        };
        let mut output = Vec::new();
        let mut warnings = Vec::new();

        write_response(&response, &mut output, &mut warnings)
            .expect("the typed response should be writable");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] USB Portable Printer
    status: new
    manufacturer: YICHIP3121
    transport: usb
    usb: 0416:5011; bus 003 address 60; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300001
[2] counter
    status: configured
    model: USB Portable Printer
    profile: unassigned
    transport: usb
    usb: 0416:5011; bus 004 address 61; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300002
[3] 10.42.0.5:9100
    status: new
    transport: network
    network: 10.42.0.5:9100
[4] kitchen
    status: configured
    profile: TM-T88V
    transport: network
    network: [2001:db8::5]:9100
    interface: enx0
    also configured as: kitchen-spare
"
        );
        assert_eq!(
            String::from_utf8(warnings).expect("the warnings should be UTF-8"),
            "Warning: could not inspect the active configuration of USB device 0416:5012: device is not configured\n"
        );
    }

    #[test]
    fn empty_typed_response_is_a_successful_snapshot() {
        let response = empty_response(Vec::new());
        let mut output = Vec::new();
        let mut warnings = Vec::new();

        write_response(&response, &mut output, &mut warnings)
            .expect("the empty response should be writable");

        assert_eq!(output, b"No printers discovered.\n");
        assert!(warnings.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn typed_permission_failures_append_one_grant_hint() {
        let response = empty_response(vec![
            UsbEnumerationFailure {
                stage: UsbFailureStage::OpenDevice,
                vendor_id: 0x0416,
                product_id: 0x5012,
                reason: "permission denied (errno 13)".to_owned(),
                permission_denied: true,
            },
            UsbEnumerationFailure {
                stage: UsbFailureStage::OpenDevice,
                vendor_id: 0x0416,
                product_id: 0x5013,
                reason: "permission denied (errno 13)".to_owned(),
                permission_denied: true,
            },
        ]);
        let mut output = Vec::new();
        let mut warnings = Vec::new();

        write_response(&response, &mut output, &mut warnings)
            .expect("the partial response should be writable");

        assert_eq!(
            String::from_utf8(warnings).expect("the warnings should be UTF-8"),
            "\
Warning: could not open USB device 0416:5012: permission denied (errno 13)
Warning: could not open USB device 0416:5013: permission denied (errno 13)
Fix USB permissions with: sudo escpost printers grant-usb-permissions
"
        );
    }

    #[test]
    fn configured_names_preserve_configuration_order() {
        let configuration = PrinterConfiguration::parse(
            r#"
[kitchen]
transport = "network"
host = "10.42.0.71"
port = 9100

[kitchen-spare]
transport = "network"
host = "10.42.0.71"
port = 9100
"#,
        )
        .expect("the printer configuration should parse");
        let host = DiscoveredHost {
            address: Ipv4Addr::new(10, 42, 0, 71),
            port: 9100,
            interface: None,
        };

        assert_eq!(
            configured_names(&configuration, &host),
            vec!["kitchen", "kitchen-spare"]
        );
    }

    #[test]
    fn registration_hints_cover_each_typed_availability_shape() {
        assert_eq!(combined_registration_hint(false, false, 9100), None);
        assert_eq!(
            combined_registration_hint(true, false, 9100),
            Some(
                "Register a new USB printer with: escpost printers add <NAME> --transport usb"
                    .to_owned()
            )
        );
        assert_eq!(
            combined_registration_hint(false, true, 9100),
            Some(
                "Register a new network printer with: escpost printers add <NAME> --transport network --discover"
                    .to_owned()
            )
        );
        assert_eq!(
            combined_registration_hint(false, true, 9200),
            Some(
                "Register a new network printer with: escpost printers add <NAME> --transport network --discover --port 9200"
                    .to_owned()
            )
        );
        assert_eq!(
            combined_registration_hint(true, true, 9200),
            Some("Register a new printer with: escpost printers add <NAME>".to_owned())
        );
    }

    #[test]
    fn scan_announcement_lists_mixed_targets_and_interfaces() {
        let targets = vec![
            ScanTarget {
                subnet: Subnet::parse("10.42.0.0/24").expect("valid subnet"),
                interface: Some("enx0".to_owned()),
                excluded: Some(Ipv4Addr::new(10, 42, 0, 9)),
            },
            ScanTarget {
                subnet: Subnet::parse("192.168.50.0/24").expect("valid subnet"),
                interface: None,
                excluded: None,
            },
        ];

        assert_eq!(
            scan_announcement(&targets, 9100),
            "Scanning 2 networks on port 9100:\n  - 10.42.0.0/24 (enx0)\n  - 192.168.50.0/24"
        );
    }

    #[test]
    fn scan_announcement_uses_the_singular_for_one_target() {
        let targets = vec![ScanTarget {
            subnet: Subnet::parse("10.42.0.0/24").expect("valid subnet"),
            interface: None,
            excluded: None,
        }];

        assert_eq!(
            scan_announcement(&targets, 9200),
            "Scanning 1 network on port 9200:\n  - 10.42.0.0/24"
        );
    }

    fn usb_printer(
        bus: &str,
        address: u8,
        serial_number: Option<&str>,
        manufacturer: Option<&str>,
    ) -> UsbPrinter {
        UsbPrinter {
            vendor_id: 0x0416,
            product_id: 0x5011,
            bus: bus.to_owned(),
            address,
            manufacturer: manufacturer.map(str::to_owned),
            product: Some("USB Portable Printer".to_owned()),
            serial_number: serial_number.map(str::to_owned),
            interface_number: 0,
            out_endpoints: vec![0x01],
            in_endpoints: vec![0x81],
        }
    }

    fn empty_response(usb_failures: Vec<UsbEnumerationFailure>) -> Response {
        Response {
            config_path: PathBuf::from("/tmp/printers.toml"),
            scan_targets: Vec::new(),
            usb_printers: Vec::new(),
            network_printers: Vec::new(),
            usb_failures,
            registration: RegistrationAvailability::default(),
        }
    }
}
