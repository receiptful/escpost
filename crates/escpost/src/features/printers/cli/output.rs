//! Shared block writers and formatting helpers for the USB and network
//! printer listings printed by both `printers list` and `printers discover`.

use std::io::Write;

use crate::error::CliError;

use super::super::inventory::UsbPrinter;

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
pub(crate) struct UsbListing<'a> {
    pub(crate) heading: &'a str,
    pub(crate) status: &'a str,
    pub(crate) model: Option<&'a str>,
    pub(crate) profile: Option<Option<&'a str>>,
    pub(crate) printer: &'a UsbPrinter,
}
pub(crate) fn write_usb_listing(
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
/// A network printer entry as shown by both `printers list` and `printers
/// discover`, so the two commands cannot drift apart. `profile` distinguishes
/// "no profile line at all" (a freshly discovered, unconfigured host) from
/// "print the line, falling back to `unassigned`" (a configured printer).
pub(crate) struct NetworkListing<'a> {
    pub(crate) heading: &'a str,
    pub(crate) status: &'a str,
    pub(crate) profile: Option<Option<&'a str>>,
    pub(crate) host: &'a str,
    pub(crate) port: u16,
    pub(crate) interface: Option<&'a str>,
    pub(crate) also_configured: &'a [&'a str],
}
pub(crate) fn write_network_listing(
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
pub(crate) fn format_network_endpoint(host: &str, port: u16) -> String {
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
pub(crate) fn usb_printer_label_parts(product: Option<&str>, manufacturer: Option<&str>) -> String {
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
