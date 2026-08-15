//! The `printers list` command core: merge the saved registry against what
//! is actually reachable right now and print the sorted result.

use super::super::Availability;
#[cfg(test)]
use super::super::cli::InventoryTransport;
#[cfg(test)]
use super::super::cli::output::{write_network_printer, write_printer, write_unavailable_printer};
#[cfg(test)]
use super::super::inventory::{
    ConnectedUsbEntry, MergedUsbIdentities, UsbInventory, merge_usb_identities,
};
use super::{ConnectionFacts, Response};
#[cfg(test)]
use crate::configuration::{ConfiguredNetworkPrinter, ConfiguredUsbPrinter, PrinterConfiguration};
use crate::error::CliError;
use std::io::Write;

pub(crate) fn write_response(response: &Response, output: &mut impl Write) -> Result<(), CliError> {
    if response.printers.is_empty() {
        writeln!(output, "No printers configured.").map_err(CliError::WriteHumanOutput)?;
        return Ok(());
    }
    for (offset, printer) in response.printers.iter().enumerate() {
        writeln!(output, "[{}] {}", offset + 1, printer.name)
            .map_err(CliError::WriteHumanOutput)?;
        let status = match printer.availability {
            Availability::Connected => "connected",
            Availability::Unavailable => "unavailable",
        };
        writeln!(output, "    status: {status}").map_err(CliError::WriteHumanOutput)?;
        match &printer.connection {
            ConnectionFacts::Usb(usb) => {
                if let Some(product) = &usb.product {
                    writeln!(output, "    model: {product}").map_err(CliError::WriteHumanOutput)?;
                }
                if let Some(manufacturer) = &usb.manufacturer {
                    writeln!(output, "    manufacturer: {manufacturer}")
                        .map_err(CliError::WriteHumanOutput)?;
                }
                writeln!(
                    output,
                    "    profile: {}",
                    printer.profile.as_deref().unwrap_or("unassigned")
                )
                .map_err(CliError::WriteHumanOutput)?;
                writeln!(output, "    transport: usb").map_err(CliError::WriteHumanOutput)?;
                if let (Some(bus), Some(address)) = (&usb.bus, usb.address) {
                    writeln!(
                        output,
                        "    usb: {:04x}:{:04x}; bus {bus} address {address}; interface {}",
                        usb.vendor_id, usb.product_id, usb.interface_number
                    )
                    .map_err(CliError::WriteHumanOutput)?;
                } else {
                    writeln!(
                        output,
                        "    usb: {:04x}:{:04x}; interface {}",
                        usb.vendor_id, usb.product_id, usb.interface_number
                    )
                    .map_err(CliError::WriteHumanOutput)?;
                }
                write!(
                    output,
                    "    endpoints: out {}",
                    format_endpoints(&usb.out_endpoints)
                )
                .map_err(CliError::WriteHumanOutput)?;
                if !usb.in_endpoints.is_empty() {
                    write!(output, "; in {}", format_endpoints(&usb.in_endpoints))
                        .map_err(CliError::WriteHumanOutput)?;
                }
                writeln!(output).map_err(CliError::WriteHumanOutput)?;
                if let Some(serial) = &usb.serial_number {
                    writeln!(output, "    serial: {serial}").map_err(CliError::WriteHumanOutput)?;
                }
            }
            ConnectionFacts::Network(network) => {
                writeln!(
                    output,
                    "    profile: {}",
                    printer.profile.as_deref().unwrap_or("unassigned")
                )
                .map_err(CliError::WriteHumanOutput)?;
                writeln!(output, "    transport: network").map_err(CliError::WriteHumanOutput)?;
                writeln!(
                    output,
                    "    network: {}",
                    super::super::cli::output::format_network_endpoint(&network.host, network.port)
                )
                .map_err(CliError::WriteHumanOutput)?;
            }
        }
    }
    Ok(())
}

fn format_endpoints(endpoints: &[u8]) -> String {
    endpoints
        .iter()
        .map(|endpoint| format!("{endpoint:#04x}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
struct ListedPrinter<'a> {
    display_name: String,
    kind: ListedPrinterKind<'a>,
}
#[cfg(test)]
enum ListedPrinterKind<'a> {
    ConnectedUsb(&'a ConnectedUsbEntry),
    UnavailableUsb(&'a ConfiguredUsbPrinter),
    Network {
        printer: &'a ConfiguredNetworkPrinter,
        connected: bool,
    },
}
/// The pure core of `printers list`: a registry-only inventory of configured
/// USB and network printers, each cross-checked against what is actually
/// reachable right now (`merge_usb_identities`'s metadata-only presence
/// check for USB, `network_statuses`'s TCP probe for network). A connected
/// USB device that matches no saved identity is never shown here — that is
/// `printers discover`'s job — so the merge is used only to resolve status
/// for entries that are already in `printers.toml`. USB presence never opens
/// a device: when no USB printers are configured at all, `inventory.
/// identities()` is not even called, so `list` is structurally incapable of
/// hitting a device-open permission error the way `discover` or `add` can.
#[cfg(test)]
fn execute(
    inventory: &mut impl UsbInventory,
    configuration: &PrinterConfiguration,
    network_statuses: &[bool],
    transport: Option<InventoryTransport>,
    output: &mut impl Write,
) -> Result<(), CliError> {
    let identities = if transport == Some(InventoryTransport::Network)
        || configuration.usb_printers().is_empty()
    {
        Vec::new()
    } else {
        inventory.identities()?
    };
    let listing = merge_usb_identities(identities, configuration);
    let mut printers = listed_printers(
        &listing,
        configuration,
        network_statuses,
        transport != Some(InventoryTransport::Usb),
    );
    if printers.is_empty() {
        writeln!(output, "No printers configured.").map_err(CliError::WriteHumanOutput)?;
        return Ok(());
    }

    printers.sort_by(|left, right| {
        left.status_rank()
            .cmp(&right.status_rank())
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
            .then_with(|| left.display_name.cmp(&right.display_name))
            .then_with(|| left.transport_rank().cmp(&right.transport_rank()))
    });
    for (offset, printer) in printers.into_iter().enumerate() {
        match printer.kind {
            ListedPrinterKind::ConnectedUsb(connected) => {
                let configured = &configuration.usb_printers()[connected.configuration_index];
                write_printer(output, offset + 1, &connected.printer, configured)?;
            }
            ListedPrinterKind::UnavailableUsb(printer) => {
                write_unavailable_printer(output, offset + 1, printer)?;
            }
            ListedPrinterKind::Network { printer, connected } => {
                write_network_printer(output, offset + 1, printer, connected)?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
fn listed_printers<'a>(
    usb: &'a MergedUsbIdentities,
    configuration: &'a PrinterConfiguration,
    network_statuses: &[bool],
    include_network: bool,
) -> Vec<ListedPrinter<'a>> {
    let mut printers = Vec::new();
    // `printers list` is the registry, not a discovery tool: a connected USB
    // device that matches no saved identity is `printers discover`'s
    // business now, so `merge_usb_identities` has already dropped it before
    // this function ever sees it.
    for connected in &usb.connected {
        let configured = &configuration.usb_printers()[connected.configuration_index];
        printers.push(ListedPrinter {
            display_name: configured.name.clone(),
            kind: ListedPrinterKind::ConnectedUsb(connected),
        });
    }
    for index in &usb.unavailable_configuration_indexes {
        let printer = &configuration.usb_printers()[*index];
        printers.push(ListedPrinter {
            display_name: printer.name.clone(),
            kind: ListedPrinterKind::UnavailableUsb(printer),
        });
    }
    if include_network {
        for (index, printer) in configuration.network_printers().iter().enumerate() {
            printers.push(ListedPrinter {
                display_name: printer.name.clone(),
                kind: ListedPrinterKind::Network {
                    printer,
                    connected: network_statuses.get(index).copied().unwrap_or(false),
                },
            });
        }
    }
    printers
}
#[cfg(test)]
impl ListedPrinter<'_> {
    fn status_rank(&self) -> u8 {
        match self.kind {
            ListedPrinterKind::ConnectedUsb(_)
            | ListedPrinterKind::Network {
                connected: true, ..
            } => 0,
            ListedPrinterKind::UnavailableUsb(_)
            | ListedPrinterKind::Network {
                connected: false, ..
            } => 1,
        }
    }

    fn transport_rank(&self) -> u8 {
        match self.kind {
            ListedPrinterKind::ConnectedUsb(_) | ListedPrinterKind::UnavailableUsb(_) => 0,
            ListedPrinterKind::Network { .. } => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::inventory::{UsbDeviceIdentity, UsbPrinter};
    use super::super::super::test_support::{FixedInventory, netum_usb_printer};
    use super::*;
    use crate::configuration::PrinterConfiguration;

    #[test]
    fn list_shows_the_usb_coordinates_needed_by_print() {
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
        // `list` only shows configured printers now, so the coordinates a
        // connected USB interface needs for `print` must come from a
        // CONFIGURED entry's merged block, not a bare unconfigured device.
        let configuration = PrinterConfiguration::parse(
            "\
[netum-usb]
transport = \"usb\"
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

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] netum-usb
    status: connected
    model: USB Portable Printer
    manufacturer: YICHIP3121
    profile: unassigned
    transport: usb
    usb: 0416:5011; bus 3 address 57; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300001
"
        );
    }

    #[test]
    fn a_connected_but_unconfigured_usb_printer_is_not_listed() {
        // Discovery duty moved entirely to `printers discover`: a connected
        // USB interface that matches no saved identity must not produce a
        // block in `list`, even though it would previously have appeared
        // under its descriptor-derived label. The configuration here has no
        // USB printers at all, so this also exercises requirement 1's
        // enumeration skip (`identities()` is never called); see
        // `a_connected_but_unconfigured_usb_identity_is_not_listed_alongside_a_configured_entry`
        // below for the case where USB *is* configured but this particular
        // device still does not match any of it.
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

        execute(
            &mut inventory,
            &PrinterConfiguration::default(),
            &[],
            None,
            &mut output,
        )
        .expect("listing should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "No printers configured.\n"
        );
    }

    #[test]
    fn empty_list_is_a_successful_snapshot() {
        let mut inventory = FixedInventory {
            printers: Vec::new(),
        };
        let mut output = Vec::new();

        execute(
            &mut inventory,
            &PrinterConfiguration::default(),
            &[],
            None,
            &mut output,
        )
        .expect("an empty listing should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "No printers configured.\n"
        );
    }

    #[test]
    fn configured_printer_is_listed_when_it_is_unavailable() {
        let mut inventory = FixedInventory {
            printers: Vec::new(),
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

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] netum-usb
    status: unavailable
    profile: NT-5890K
    transport: usb
    usb: 0416:5011; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300001
"
        );
    }

    #[test]
    fn configured_usb_printer_can_remain_unprofiled() {
        let mut inventory = FixedInventory {
            printers: Vec::new(),
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

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        assert!(
            String::from_utf8(output)
                .expect("the listing should be UTF-8")
                .contains("profile: unassigned")
        );
    }

    #[test]
    fn connected_configured_printer_is_merged_into_one_named_entry() {
        let mut inventory = FixedInventory {
            printers: vec![UsbPrinter {
                vendor_id: 0x0416,
                product_id: 0x5011,
                bus: "3".to_owned(),
                address: 57,
                manufacturer: None,
                product: Some("USB Portable Printer".to_owned()),
                serial_number: Some("B120300001".to_owned()),
                interface_number: 0,
                out_endpoints: vec![0x01],
                in_endpoints: vec![0x81],
            }],
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

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] netum-usb
    status: connected
    model: USB Portable Printer
    profile: NT-5890K
    transport: usb
    usb: 0416:5011; bus 3 address 57; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300001
"
        );
    }

    #[test]
    fn connected_printers_sort_first_then_each_status_sorts_by_display_name() {
        let mut inventory = FixedInventory {
            printers: vec![
                UsbPrinter {
                    vendor_id: 0x1000,
                    product_id: 0x0001,
                    bus: "1".to_owned(),
                    address: 1,
                    manufacturer: None,
                    product: Some("Zed Model".to_owned()),
                    serial_number: Some("CONNECTED".to_owned()),
                    interface_number: 0,
                    out_endpoints: vec![0x01],
                    in_endpoints: Vec::new(),
                },
                UsbPrinter {
                    vendor_id: 0x2000,
                    product_id: 0x0002,
                    bus: "2".to_owned(),
                    address: 2,
                    manufacturer: None,
                    product: Some("Alpha Model".to_owned()),
                    serial_number: None,
                    interface_number: 0,
                    out_endpoints: vec![0x01],
                    in_endpoints: Vec::new(),
                },
            ],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[Zulu]
transport = \"usb\"
profile = \"CONNECTED\"
vendor_id = \"0x1000\"
product_id = \"0x0001\"
serial_number = \"CONNECTED\"
interface_number = 0
out_endpoint = \"0x01\"

[Alpha]
transport = \"usb\"
profile = \"CONNECTED-ALPHA\"
vendor_id = \"0x2000\"
product_id = \"0x0002\"
interface_number = 0
out_endpoint = \"0x01\"

[charlie]
transport = \"usb\"
profile = \"OFFLINE-C\"
vendor_id = \"0x3000\"
product_id = \"0x0003\"
interface_number = 0
out_endpoint = \"0x01\"

[Bravo]
transport = \"usb\"
profile = \"OFFLINE-B\"
vendor_id = \"0x4000\"
product_id = \"0x0004\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        let headings = String::from_utf8(output)
            .expect("the listing should be UTF-8")
            .lines()
            .filter(|line| line.starts_with('['))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(
            headings,
            vec!["[1] Alpha", "[2] Zulu", "[3] Bravo", "[4] charlie",]
        );
    }

    #[test]
    fn one_saved_identity_names_at_most_one_connected_interface() {
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

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        // Both connected devices share one USB identity, but only one
        // configured entry claims it (`merge_usb_identities`, first-match by
        // stable location). The other device is left unconfigured, and
        // `list` no longer shows connected-but-unconfigured devices at all,
        // so it must produce no second block ("Second Model" never appears).
        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert_eq!(output.matches("] shared-identity\n").count(), 1);
        assert_eq!(output.matches("status: connected").count(), 1);
        assert!(!output.contains("status: unavailable"));
        assert!(!output.contains("Second Model"));
    }

    #[test]
    fn a_connected_but_unconfigured_usb_identity_is_not_listed_alongside_a_configured_entry() {
        // Unlike `a_connected_but_unconfigured_usb_printer_is_not_listed`,
        // the configuration here is not empty, so `identities()` genuinely
        // runs; this proves the non-matching identity is dropped by
        // `merge_usb_identities` itself rather than by requirement 1's
        // enumeration skip.
        let mut inventory = FixedInventory {
            printers: vec![
                netum_usb_printer(vec![0x01], vec![0x81]),
                UsbPrinter {
                    vendor_id: 0x9999,
                    product_id: 0x0001,
                    bus: "9".to_owned(),
                    address: 9,
                    manufacturer: None,
                    product: Some("Stranger Printer".to_owned()),
                    serial_number: None,
                    interface_number: 0,
                    out_endpoints: vec![0x01],
                    in_endpoints: Vec::new(),
                },
            ],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[netum-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
serial_number = \"B120300001\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert!(output.contains("] netum-usb\n"));
        assert!(output.contains("status: connected"));
        assert!(
            !output.contains("Stranger Printer"),
            "an identity matching no saved USB printer must never appear in `list`:\n{output}"
        );
    }

    #[test]
    fn list_first_match_wins_between_two_configured_entries_sharing_one_identity() {
        // The "vice versa" half of first-match-wins: two configured entries
        // both matching the *same* ambiguous pair of connected devices (no
        // serial on either side) must still produce exactly one connected
        // block. The losing configured entry is claimed by the ambiguity
        // resolution too, so it is neither connected nor unavailable —
        // mirroring `classify_usb_printers`' own handling of this case for
        // `printers discover`.
        let mut first_device = netum_usb_printer(vec![0x01], vec![0x81]);
        first_device.serial_number = None;
        first_device.bus = "1".to_owned();
        let mut second_device = first_device.clone();
        second_device.bus = "2".to_owned();
        let mut inventory = FixedInventory {
            printers: vec![first_device, second_device],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[first-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
interface_number = 0
out_endpoint = \"0x01\"

[second-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert_eq!(output.matches("] first-usb\n").count(), 1);
        assert_eq!(output.matches("status: connected").count(), 1);
        assert!(
            !output.contains("second-usb"),
            "the losing configured entry must not appear as connected or unavailable:\n{output}"
        );
    }

    #[test]
    fn list_configured_without_serial_matches_a_connected_serial_and_prefers_it() {
        let mut inventory = FixedInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[unserialized-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("an unserialized configuration should be valid");
        let mut output = Vec::new();

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert!(output.contains("status: connected"));
        assert!(
            output.contains("serial: B120300001"),
            "the connected device's own serial should be shown even though the saved entry has none:\n{output}"
        );
    }

    #[test]
    fn list_configured_serial_must_equal_the_connected_serial() {
        let mut inventory = FixedInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[mismatched-serial-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
serial_number = \"SOME-OTHER-SERIAL\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert!(
            output.contains("status: unavailable"),
            "a differing saved serial must not match the connected device:\n{output}"
        );
        assert!(!output.contains("status: connected"));
    }

    #[test]
    fn list_omits_the_serial_line_when_neither_side_has_one() {
        let mut printer = netum_usb_printer(vec![0x01], vec![0x81]);
        printer.serial_number = None;
        let mut inventory = FixedInventory {
            printers: vec![printer],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[unserialized-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("an unserialized configuration should be valid");
        let mut output = Vec::new();

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert!(output.contains("status: connected"));
        assert!(!output.contains("serial:"));
    }

    #[test]
    fn list_omits_the_model_line_when_the_identity_has_no_product_string() {
        let mut printer = netum_usb_printer(vec![0x01], vec![0x81]);
        printer.product = None;
        printer.manufacturer = None;
        let mut inventory = FixedInventory {
            printers: vec![printer],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[netum-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
serial_number = \"B120300001\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        let output = String::from_utf8(output).expect("the listing should be UTF-8");
        assert!(output.contains("status: connected"));
        assert!(
            !output.contains("model:"),
            "no product string means no model line, matching `write_usb_listing`'s own `model: None` handling:\n{output}"
        );
    }

    #[test]
    fn list_sources_interface_and_endpoints_from_configuration_not_the_device() {
        // `UsbDeviceIdentity` carries no interface or endpoint fields at
        // all, so this is also a type-level guarantee; this test pins the
        // observable behavior against a configuration whose interface and
        // endpoints are deliberately unlike the usual fixtures.
        let mut inventory = FixedInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
        };
        let configuration = PrinterConfiguration::parse(
            "\
[netum-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
serial_number = \"B120300001\"
interface_number = 3
out_endpoint = \"0x05\"
in_endpoint = \"0x86\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute(&mut inventory, &configuration, &[], None, &mut output)
            .expect("listing should succeed");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] netum-usb
    status: connected
    model: USB Portable Printer
    manufacturer: YICHIP3121
    profile: unassigned
    transport: usb
    usb: 0416:5011; bus 003 address 60; interface 3
    endpoints: out 0x05; in 0x86
    serial: B120300001
"
        );
    }

    #[test]
    fn list_skips_usb_enumeration_entirely_when_no_usb_printers_are_configured() {
        // Structural proof of requirement 1: a double whose `list()` and
        // `identities()` both panic proves `execute` never touches USB at
        // all when `configuration.usb_printers()` is empty, even though the
        // registry is not otherwise empty (a network printer is configured).
        struct PanicsIfUsbIsQueried;
        impl UsbInventory for PanicsIfUsbIsQueried {
            fn list(&mut self) -> Result<Vec<UsbPrinter>, CliError> {
                panic!("list() must not run when no USB printers are configured");
            }

            fn identities(&mut self) -> Result<Vec<UsbDeviceIdentity>, CliError> {
                panic!("identities() must not run when no USB printers are configured");
            }
        }
        let configuration = PrinterConfiguration::parse(
            r#"
[kitchen]
transport = "network"
host = "10.42.0.71"
port = 9100
"#,
        )
        .expect("the network-only configuration should parse");
        let mut output = Vec::new();

        execute(
            &mut PanicsIfUsbIsQueried,
            &configuration,
            &[false],
            None,
            &mut output,
        )
        .expect("listing should succeed without touching USB at all");

        assert!(
            String::from_utf8(output)
                .expect("the listing should be UTF-8")
                .contains("] kitchen")
        );
    }

    #[test]
    fn list_never_opens_usb_devices_to_check_presence() {
        // Structural proof of requirement 2: a double whose `list()` panics
        // but whose `identities()` succeeds proves `execute` resolves USB
        // presence purely from metadata, the same way
        // `NusbInventory::identities` never calls `.open()`.
        struct MetadataOnlyInventory;
        impl UsbInventory for MetadataOnlyInventory {
            fn list(&mut self) -> Result<Vec<UsbPrinter>, CliError> {
                panic!("printers list must never call the open-based list()");
            }

            fn identities(&mut self) -> Result<Vec<UsbDeviceIdentity>, CliError> {
                Ok(vec![UsbDeviceIdentity {
                    vendor_id: 0x0416,
                    product_id: 0x5011,
                    bus: "3".to_owned(),
                    address: 57,
                    manufacturer: Some("YICHIP3121".to_owned()),
                    product: Some("USB Portable Printer".to_owned()),
                    serial_number: Some("B120300001".to_owned()),
                }])
            }
        }
        let configuration = PrinterConfiguration::parse(
            "\
[netum-usb]
transport = \"usb\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
serial_number = \"B120300001\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("the printer configuration should be valid");
        let mut output = Vec::new();

        execute(
            &mut MetadataOnlyInventory,
            &configuration,
            &[],
            None,
            &mut output,
        )
        .expect("listing should succeed without opening any device");

        assert!(
            String::from_utf8(output)
                .expect("the listing should be UTF-8")
                .contains("status: connected")
        );
    }
}
