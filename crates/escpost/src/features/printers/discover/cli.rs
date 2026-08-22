//! The `printers discover` terminal adapter.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use super::super::cli::output::{
    NetworkListing, UsbListing, format_network_endpoint, usb_printer_label_parts,
    write_network_listing, write_usb_listing,
};
use super::super::cli::scan_announcement;
use super::super::cli::{DiscoverPrintersArgs, InventoryTransport};
use super::super::inventory::{NusbInventory, UsbEnumerationFailure, UsbFailureStage};
use super::{DiscoveryEvent, DiscoveryScope, NetworkScan, Response, execute, prepare};
use crate::discovery::SkippedInterface;
use crate::error::CliError;

impl TryFrom<DiscoverPrintersArgs> for DiscoveryScope {
    type Error = CliError;

    fn try_from(arguments: DiscoverPrintersArgs) -> Result<Self, Self::Error> {
        if arguments.transport == Some(InventoryTransport::Usb) {
            if !arguments.subnet.is_empty()
                || arguments.port.is_some()
                || arguments.timeout.is_some()
            {
                return Err(CliError::NetworkScanOptionForUsbDiscovery);
            }
            return Ok(Self::Usb);
        }
        let scan = NetworkScan::new(
            arguments.port.unwrap_or(9100),
            arguments.subnet,
            Duration::from_millis(arguments.timeout.unwrap_or(1000)),
        )?;
        Ok(match arguments.transport {
            Some(InventoryTransport::Network) => Self::Network(scan),
            None => Self::All(scan),
            Some(InventoryTransport::Usb) => unreachable!("USB was handled above"),
        })
    }
}

/// The terminal's line for one skipped adapter: the shared reason, then the
/// flag that scans it anyway. The remedy is composed here rather than carried
/// by `SkippedInterface` because it is the terminal's alone — the workbench
/// answers the same omission by pointing at its custom-network field.
///
/// Shared with `printers add --discover`, which runs the same scan and so
/// must report the same omissions in the same words.
pub(in crate::features::printers) fn skipped_line(adapter: &SkippedInterface) -> String {
    match adapter.cli_hint() {
        Some(hint) => format!("Skipped {}, {hint}", adapter.describe()),
        None => format!("Skipped {}", adapter.describe()),
    }
}

pub(crate) async fn run_discover(
    arguments: DiscoverPrintersArgs,
    config: Option<PathBuf>,
) -> Result<(), CliError> {
    let scope = DiscoveryScope::try_from(arguments)?;
    let port = scope.network_scan().map_or(9100, NetworkScan::port);
    let prepared = prepare(config, scope)?;
    let bar = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::stderr());
    bar.set_style(
        ProgressStyle::with_template("{msg} [{bar:40}] {pos}/{len}")
            .expect("the progress bar template is a compile-time constant")
            .progress_chars("=> "),
    );
    bar.set_message("Scanning for network printers");
    let mut length_set = false;
    let response = execute(
        prepared,
        |event| match event {
            DiscoveryEvent::Prepared {
                config_path,
                scope,
                scan_targets,
                skipped,
            } => {
                eprintln!("Reading configuration from {}", config_path.display());
                if let Some(scan) = scope.network_scan() {
                    // Printed whenever an adapter was skipped, even if nothing is
                    // left to scan: a combined USB+network discovery still has USB
                    // work to do, and the omission must be reported either way.
                    for adapter in skipped {
                        eprintln!("{}", skipped_line(adapter));
                    }
                    if !scan_targets.is_empty() {
                        eprintln!("{}", scan_announcement(scan_targets, scan.port()));
                        if scan.uses_automatic_subnets() {
                            eprintln!("Tip: pass --subnet <CIDR> to scan a different network.");
                        }
                    }
                }
            }
            // The CLI still renders from the final `Response` once discovery
            // finishes, so a live result here needs no immediate handling —
            // only the progress bar reacts as the sweep runs.
            DiscoveryEvent::UsbPrinter(_)
            | DiscoveryEvent::UsbFailure(_)
            | DiscoveryEvent::NetworkPrinter(_) => {}
            DiscoveryEvent::NetworkScanProgress { completed, total } => {
                if !length_set {
                    bar.set_length(total);
                    length_set = true;
                }
                bar.set_position(completed);
            }
        },
        &mut NusbInventory,
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
    use crate::application::ApplicationError;
    use crate::discovery::ScanTarget;
    use crate::discovery::{SkipReason, Subnet};
    use crate::features::printers::discover::{
        DiscoveryScope, NetworkDiscovery, NetworkScan, RegistrationAvailability, UsbDiscovery,
    };
    use crate::features::printers::inventory::UsbPrinter;

    /// Moving the remedy out of `SkippedInterface::describe` must leave the
    /// terminal saying exactly what it said before, flag included.
    #[test]
    fn the_skipped_line_still_names_the_flag_that_scans_the_adapter() {
        let too_large = SkippedInterface {
            name: "enp5s0".to_owned(),
            subnet: Some(Subnet::parse("10.0.0.0/16").expect("valid subnet")),
            reason: SkipReason::TooLarge,
        };
        let unusable = SkippedInterface {
            name: "weird0".to_owned(),
            subnet: None,
            reason: SkipReason::UnusableNetmask,
        };

        assert_eq!(
            skipped_line(&too_large),
            "Skipped enp5s0 (10.0.0.0/16): larger than /24, scan it with --subnet 10.0.0.0/16"
        );
        assert_eq!(
            skipped_line(&unusable),
            "Skipped weird0: its netmask does not name a scannable subnet"
        );
    }

    #[test]
    fn cli_arguments_convert_to_each_valid_discovery_scope() {
        let subnet = Subnet::parse("10.42.0.71/24").expect("valid subnet");
        let cases = [
            (
                DiscoverPrintersArgs {
                    transport: Some(InventoryTransport::Usb),
                    port: None,
                    subnet: Vec::new(),
                    timeout: None,
                },
                DiscoveryScope::Usb,
            ),
            (
                DiscoverPrintersArgs {
                    transport: Some(InventoryTransport::Network),
                    port: Some(9200),
                    subnet: vec![subnet],
                    timeout: Some(75),
                },
                DiscoveryScope::Network(
                    NetworkScan::new(9200, vec![subnet], Duration::from_millis(75))
                        .expect("the expected network scan should be valid"),
                ),
            ),
            (
                DiscoverPrintersArgs {
                    transport: None,
                    port: None,
                    subnet: Vec::new(),
                    timeout: None,
                },
                DiscoveryScope::All(
                    NetworkScan::new(9100, Vec::new(), Duration::from_millis(1000))
                        .expect("the expected combined scan should be valid"),
                ),
            ),
        ];

        for (arguments, expected) in cases {
            assert_eq!(DiscoveryScope::try_from(arguments).unwrap(), expected);
        }
    }

    #[test]
    fn every_network_option_combination_is_rejected_for_usb_discovery() {
        let subnet = Subnet::parse("127.0.0.1/32").expect("valid subnet");
        for option_mask in 1u8..=7 {
            let arguments = DiscoverPrintersArgs {
                transport: Some(InventoryTransport::Usb),
                port: (option_mask & 0b001 != 0).then_some(9100),
                subnet: if option_mask & 0b010 != 0 {
                    vec![subnet]
                } else {
                    Vec::new()
                },
                timeout: (option_mask & 0b100 != 0).then_some(1000),
            };

            assert!(matches!(
                DiscoveryScope::try_from(arguments),
                Err(CliError::NetworkScanOptionForUsbDiscovery)
            ));
        }
    }

    #[test]
    fn zero_port_is_rejected_while_converting_network_and_all_scopes() {
        for transport in [Some(InventoryTransport::Network), None] {
            let error = DiscoveryScope::try_from(DiscoverPrintersArgs {
                transport,
                port: Some(0),
                subnet: Vec::new(),
                timeout: None,
            })
            .expect_err("zero is not a valid network discovery port");

            assert!(matches!(
                error,
                CliError::Application(ApplicationError::InvalidPrinterPort)
            ));
        }
    }

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
                excluded: vec![Ipv4Addr::new(10, 42, 0, 9)],
            },
            ScanTarget {
                subnet: Subnet::parse("192.168.50.0/24").expect("valid subnet"),
                interface: None,
                excluded: Vec::new(),
            },
        ];

        assert_eq!(
            scan_announcement(&targets, 9100),
            "Scanning 2 networks on port 9100 (507 addresses):\n  - 10.42.0.0/24 (enx0)\n  - 192.168.50.0/24"
        );
    }

    #[test]
    fn scan_announcement_uses_the_singular_for_one_target() {
        let targets = vec![ScanTarget {
            subnet: Subnet::parse("10.42.0.0/24").expect("valid subnet"),
            interface: None,
            excluded: Vec::new(),
        }];

        assert_eq!(
            scan_announcement(&targets, 9200),
            "Scanning 1 network on port 9200 (254 addresses):\n  - 10.42.0.0/24"
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
