//! Shared block writers and formatting helpers for the USB and network
//! printer listings printed by both `printers list` and `printers discover`.

use std::io::Write;

#[cfg(test)]
use crate::configuration::{ConfiguredNetworkPrinter, ConfiguredUsbPrinter};
use crate::error::CliError;

use super::inventory::UsbPrinter;

const UNASSIGNED_PROFILE: &str = "unassigned";
/// A USB printer entry as shown by both `printers list` and `printers
/// discover`, mirroring `NetworkListing` below so the two commands cannot
/// drift apart. `model` distinguishes "no model line" (an unconfigured
/// connected printer) from "print the model line" (a configured printer);
/// either way it is the product string alone, never combined with the
/// manufacturer. The `manufacturer:` line below it is driven directly by
/// `printer.manufacturer` rather than by a field on this struct, so it
/// appears whenever the device reports one, independent of whether `model`
/// itself is shown. `profile` distinguishes "no profile line at all" (a
/// freshly discovered, unconfigured printer on `discover`) from "print the
/// line, falling back to `unassigned`" (a configured printer on either
/// command, or an unconfigured but connected printer on `list`).
pub(super) struct UsbListing<'a> {
    pub(super) heading: &'a str,
    pub(super) status: &'a str,
    pub(super) model: Option<&'a str>,
    pub(super) profile: Option<Option<&'a str>>,
    pub(super) printer: &'a UsbPrinter,
}
pub(super) fn write_usb_listing(
    output: &mut impl Write,
    number: usize,
    listing: &UsbListing<'_>,
) -> Result<(), CliError> {
    writeln!(output, "[{number}] {}", listing.heading).map_err(CliError::WriteHumanOutput)?;
    writeln!(output, "    status: {}", listing.status).map_err(CliError::WriteHumanOutput)?;
    if let Some(model) = listing.model {
        writeln!(output, "    model: {model}").map_err(CliError::WriteHumanOutput)?;
    }
    if let Some(manufacturer) = listing.printer.manufacturer.as_deref() {
        writeln!(output, "    manufacturer: {manufacturer}").map_err(CliError::WriteHumanOutput)?;
    }
    if let Some(profile) = listing.profile {
        writeln!(
            output,
            "    profile: {}",
            profile.unwrap_or(UNASSIGNED_PROFILE)
        )
        .map_err(CliError::WriteHumanOutput)?;
    }
    writeln!(output, "    transport: usb").map_err(CliError::WriteHumanOutput)?;
    writeln!(
        output,
        "    usb: {:04x}:{:04x}; bus {} address {}; interface {}",
        listing.printer.vendor_id,
        listing.printer.product_id,
        listing.printer.bus,
        listing.printer.address,
        listing.printer.interface_number
    )
    .map_err(CliError::WriteHumanOutput)?;
    write!(
        output,
        "    endpoints: out {}",
        format_endpoints(&listing.printer.out_endpoints)
    )
    .map_err(CliError::WriteHumanOutput)?;
    if !listing.printer.in_endpoints.is_empty() {
        write!(
            output,
            "; in {}",
            format_endpoints(&listing.printer.in_endpoints)
        )
        .map_err(CliError::WriteHumanOutput)?;
    }
    writeln!(output).map_err(CliError::WriteHumanOutput)?;
    if let Some(serial_number) = &listing.printer.serial_number {
        writeln!(output, "    serial: {serial_number}").map_err(CliError::WriteHumanOutput)?;
    }
    Ok(())
}
/// Write one `list` connected-USB block. Every caller already has a matched
/// configuration (`merge_usb_identities` drops anything unmatched), so
/// `configured` is not optional here, unlike the discover-side listing. The
/// `model:` line is omitted rather than falling back to a generic label when
/// the device identity itself carries no product string, matching
/// `write_usb_listing`'s own `model: None` handling. The `manufacturer:` line
/// needs no equivalent handling here: `write_usb_listing` already sources it
/// straight from `printer.manufacturer`.
#[cfg(test)]
pub(super) fn write_printer(
    output: &mut impl Write,
    number: usize,
    printer: &UsbPrinter,
    configured: &ConfiguredUsbPrinter,
) -> Result<(), CliError> {
    write_usb_listing(
        output,
        number,
        &UsbListing {
            heading: &configured.name,
            status: "connected",
            model: printer.product.as_deref(),
            profile: Some(configured.profile.as_deref()),
            printer,
        },
    )
}
#[cfg(test)]
pub(super) fn write_unavailable_printer(
    output: &mut impl Write,
    number: usize,
    printer: &ConfiguredUsbPrinter,
) -> Result<(), CliError> {
    writeln!(output, "[{number}] {}", printer.name).map_err(CliError::WriteHumanOutput)?;
    writeln!(output, "    status: unavailable").map_err(CliError::WriteHumanOutput)?;
    writeln!(
        output,
        "    profile: {}",
        printer.profile.as_deref().unwrap_or(UNASSIGNED_PROFILE)
    )
    .map_err(CliError::WriteHumanOutput)?;
    writeln!(output, "    transport: usb").map_err(CliError::WriteHumanOutput)?;
    writeln!(
        output,
        "    usb: {:04x}:{:04x}; interface {}",
        printer.vendor_id, printer.product_id, printer.interface_number
    )
    .map_err(CliError::WriteHumanOutput)?;
    write!(output, "    endpoints: out {:#04x}", printer.out_endpoint)
        .map_err(CliError::WriteHumanOutput)?;
    if let Some(in_endpoint) = printer.in_endpoint {
        write!(output, "; in {in_endpoint:#04x}").map_err(CliError::WriteHumanOutput)?;
    }
    writeln!(output).map_err(CliError::WriteHumanOutput)?;
    if let Some(serial_number) = &printer.serial_number {
        writeln!(output, "    serial: {serial_number}").map_err(CliError::WriteHumanOutput)?;
    }
    Ok(())
}
/// A network printer entry as shown by both `printers list` and `printers
/// discover`, so the two commands cannot drift apart. `profile` distinguishes
/// "no profile line at all" (a freshly discovered, unconfigured host) from
/// "print the line, falling back to `unassigned`" (a configured printer).
pub(super) struct NetworkListing<'a> {
    pub(super) heading: &'a str,
    pub(super) status: &'a str,
    pub(super) profile: Option<Option<&'a str>>,
    pub(super) host: &'a str,
    pub(super) port: u16,
    pub(super) interface: Option<&'a str>,
    pub(super) also_configured: &'a [&'a str],
}
pub(super) fn write_network_listing(
    output: &mut impl Write,
    number: usize,
    listing: &NetworkListing<'_>,
) -> Result<(), CliError> {
    writeln!(output, "[{number}] {}", listing.heading).map_err(CliError::WriteHumanOutput)?;
    writeln!(output, "    status: {}", listing.status).map_err(CliError::WriteHumanOutput)?;
    if let Some(profile) = listing.profile {
        writeln!(
            output,
            "    profile: {}",
            profile.unwrap_or(UNASSIGNED_PROFILE)
        )
        .map_err(CliError::WriteHumanOutput)?;
    }
    writeln!(output, "    transport: network").map_err(CliError::WriteHumanOutput)?;
    writeln!(
        output,
        "    network: {}",
        format_network_endpoint(listing.host, listing.port)
    )
    .map_err(CliError::WriteHumanOutput)?;
    if let Some(interface) = listing.interface {
        writeln!(output, "    interface: {interface}").map_err(CliError::WriteHumanOutput)?;
    }
    for name in listing.also_configured {
        writeln!(output, "    also configured as: {name}").map_err(CliError::WriteHumanOutput)?;
    }
    Ok(())
}
#[cfg(test)]
pub(super) fn write_network_printer(
    output: &mut impl Write,
    number: usize,
    printer: &ConfiguredNetworkPrinter,
    connected: bool,
) -> Result<(), CliError> {
    write_network_listing(
        output,
        number,
        &NetworkListing {
            heading: &printer.name,
            status: if connected {
                "connected"
            } else {
                "unavailable"
            },
            profile: Some(printer.profile.as_deref()),
            host: &printer.host,
            port: printer.port,
            interface: None,
            also_configured: &[],
        },
    )
}
pub(super) fn format_network_endpoint(host: &str, port: u16) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
/// Combine a product string and manufacturer into the one-line label used by
/// the `add` picker's menu row and its saved-descriptor `Display` impl, where
/// screen density matters more than the `list`/`discover` blocks' split
/// `model:`/`manufacturer:` lines. Falls back to a generic product label when
/// the device reports none.
pub(super) fn usb_printer_label_parts(product: Option<&str>, manufacturer: Option<&str>) -> String {
    let product = product.unwrap_or("USB printer");
    manufacturer.map_or_else(
        || product.to_owned(),
        |value| format!("{product} ({value})"),
    )
}
fn format_endpoints(endpoints: &[u8]) -> String {
    endpoints
        .iter()
        .map(|endpoint| format!("{endpoint:#04x}"))
        .collect::<Vec<_>>()
        .join(", ")
}
