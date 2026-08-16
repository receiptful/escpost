//! `printers` command dispatch: routes `escpost printers <list|add|discover>`
//! to its submodule and exposes the two entry points the rest of the crate
//! calls (`run` from `lib.rs`, `add_interactively` from `print.rs`).

use std::io;
use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use crate::discovery::Subnet;
use crate::error::CliError;

use super::discover::cli::run_discover;
use super::{Transport, list};

#[cfg(target_os = "linux")]
mod grant_usb_permissions;
pub(super) mod output;

/// Format the factual targets prepared by discovery for terminal display.
pub(super) fn scan_announcement(targets: &[crate::discovery::ScanTarget], port: u16) -> String {
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

#[derive(Debug, Args)]
pub(crate) struct PrintersArgs {
    /// Read printer configuration from this exact file.
    #[arg(long, global = true, value_name = "FILE")]
    pub(crate) config: Option<PathBuf>,

    #[command(subcommand)]
    pub(crate) command: PrintersCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PrintersCommand {
    /// List currently usable printers.
    List(ListPrintersArgs),
    /// Register a printer in the local configuration.
    Add(AddPrinterArgs),
    /// Find connected USB printers and network printers listening on the RAW TCP port.
    Discover(DiscoverPrintersArgs),
    /// Grant the current user access to USB printers (writes a udev rule; run with sudo).
    #[cfg(target_os = "linux")]
    GrantUsbPermissions(GrantUsbPermissionsArgs),
}

#[cfg(target_os = "linux")]
#[derive(Debug, Args)]
pub(crate) struct GrantUsbPermissionsArgs {}

#[derive(Debug, Args)]
pub(crate) struct ListPrintersArgs {
    /// Show only one connection transport.
    #[arg(long, value_enum)]
    pub(crate) transport: Option<InventoryTransport>,
}

#[derive(Debug, Args)]
pub(crate) struct DiscoverPrintersArgs {
    /// Discover only one connection transport.
    #[arg(long, value_enum)]
    pub(crate) transport: Option<InventoryTransport>,
    /// Raw TCP port to probe. Defaults to 9100.
    #[arg(long)]
    pub(crate) port: Option<u16>,
    /// Scan this network (CIDR notation, for example 10.42.0.0/24) instead
    /// of the directly connected networks. May be repeated.
    #[arg(long, value_name = "CIDR", value_parser = Subnet::parse)]
    pub(crate) subnet: Vec<Subnet>,
    /// Per-host connection timeout in milliseconds. Defaults to 1000.
    #[arg(long, value_name = "MS")]
    pub(crate) timeout: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum InventoryTransport {
    Usb,
    Network,
}

#[derive(Debug, Args)]
pub(crate) struct AddPrinterArgs {
    /// Developer-assigned printer name.
    pub(crate) name: Option<String>,
    /// Connection transport.
    #[arg(long, value_enum)]
    pub(crate) transport: Option<PrinterTransport>,
    /// Network hostname or IP address.
    #[arg(long)]
    pub(crate) host: Option<String>,
    /// Raw TCP port. Defaults to 9100.
    #[arg(long)]
    pub(crate) port: Option<u16>,
    /// Select a USB printer by vendor ID (decimal or `0x`-prefixed hexadecimal).
    #[arg(long, value_parser = parse_usb_id)]
    pub(crate) vendor_id: Option<u16>,
    /// Select a USB printer by product ID (decimal or `0x`-prefixed hexadecimal).
    #[arg(long, value_parser = parse_usb_id)]
    pub(crate) product_id: Option<u16>,
    /// Select a USB printer by exact serial number.
    #[arg(long)]
    pub(crate) serial: Option<String>,
    /// Optional rendering profile.
    #[arg(long)]
    pub(crate) profile: Option<String>,
    /// Discover listening network printers and register the chosen one
    /// instead of passing --host.
    #[arg(long, conflicts_with_all = ["host", "vendor_id", "product_id", "serial"])]
    pub(crate) discover: bool,
    /// Scan this network (CIDR notation, for example 10.42.0.0/24) instead
    /// of the directly connected networks. May be repeated.
    #[arg(long, value_name = "CIDR", value_parser = Subnet::parse, requires = "discover")]
    pub(crate) subnet: Vec<Subnet>,
    /// Per-host connection timeout in milliseconds during discovery.
    #[arg(long, value_name = "MS", requires = "discover")]
    pub(crate) timeout: Option<u64>,
}

fn parse_usb_id(value: &str) -> Result<u16, String> {
    let text = value.trim();
    let parsed = match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hexadecimal) => u16::from_str_radix(hexadecimal, 16),
        None => text.parse::<u16>(),
    };
    parsed.map_err(|_| {
        format!("expected a decimal or 0x-prefixed 16-bit USB identifier, found `{value}`")
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum PrinterTransport {
    Usb,
    Network,
}

pub(crate) async fn run(arguments: PrintersArgs, non_interactive: bool) -> Result<(), CliError> {
    match arguments.command {
        PrintersCommand::List(list) => {
            let response = list::execute_with_observer(
                list::Request {
                    config: arguments.config,
                    transport: list.transport.map(transport_filter),
                },
                |path| {
                    eprintln!("Reading configuration from {}", path.display());
                },
            )
            .await?;
            list::cli::write_response(&response, &mut io::stdout().lock())?;
            // Count-independent, unlike discover's hints: there is always
            // exactly one next step worth pointing at, whether the registry
            // was empty or full.
            eprintln!("Discover connected printers with: escpost printers discover");
            Ok(())
        }
        PrintersCommand::Add(add) => {
            super::add::cli::run(arguments.config.as_deref(), add, non_interactive)
                .await
                .map(|_| ())
        }
        PrintersCommand::Discover(discover) => run_discover(discover, arguments.config).await,
        #[cfg(target_os = "linux")]
        PrintersCommand::GrantUsbPermissions(args) => {
            grant_usb_permissions::run(args, non_interactive)
        }
    }
}

fn transport_filter(transport: InventoryTransport) -> Transport {
    match transport {
        InventoryTransport::Usb => Transport::Usb,
        InventoryTransport::Network => Transport::Network,
    }
}
pub(crate) fn add_interactively(config_path: Option<&std::path::Path>) -> Result<String, CliError> {
    super::add::cli::add_interactively(config_path)
}
