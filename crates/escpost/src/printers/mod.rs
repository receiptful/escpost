//! `printers` command dispatch: routes `escpost printers <list|add|discover>`
//! to its submodule and exposes the two entry points the rest of the crate
//! calls (`run` from `lib.rs`, `add_interactively` from `print.rs`).

mod add;
mod discover;
#[cfg(target_os = "linux")]
mod grant_usb_permissions;
mod inventory;
mod list;
mod output;
#[cfg(test)]
mod test_support;

use std::io::{self, IsTerminal};

use crate::cli::{
    AddPrinterArgs, InventoryTransport, PrinterTransport, PrintersArgs, PrintersCommand,
};
use crate::configuration;
use crate::error::CliError;

/// Re-exported specifically so `error.rs` can build
/// `CliError::GrantUsbPermissionsNeedsRoot`'s message from it, the same way
/// several existing `CliError` variants already call into
/// `crate::configuration::display_path` from their own `#[error(...)]`
/// attributes.
#[cfg(target_os = "linux")]
pub(crate) use grant_usb_permissions::needs_root_guidance;

use add::{InquireAddPrompter, discover_host_for_add, execute_add};
use discover::run_discover;
use inventory::NusbInventory;
use list::{execute, probe_network_printers};

pub(crate) async fn run(arguments: PrintersArgs, non_interactive: bool) -> Result<(), CliError> {
    match arguments.command {
        PrintersCommand::List(list) => {
            let path = configuration::resolved_path(arguments.config.as_deref())?;
            let configuration = configuration::load(arguments.config.as_deref())?;
            eprintln!(
                "Reading configuration from {}",
                configuration::display_path(&path)
            );
            let network_statuses = if list.transport == Some(InventoryTransport::Usb) {
                Vec::new()
            } else {
                probe_network_printers(configuration.network_printers()).await
            };
            let mut inventory = NusbInventory;
            execute(
                &mut inventory,
                &configuration,
                &network_statuses,
                list.transport,
                &mut io::stdout().lock(),
            )?;
            // Count-independent, unlike discover's hints: there is always
            // exactly one next step worth pointing at, whether the registry
            // was empty or full.
            eprintln!("Discover connected printers with: escpost printers discover");
            Ok(())
        }
        PrintersCommand::Add(mut add) => {
            if add.discover {
                if add.transport == Some(PrinterTransport::Usb) {
                    return Err(CliError::DiscoverForUsbPrinter);
                }
                // Discovery implies the network transport, so the wizard must
                // not ask for one.
                add.transport = Some(PrinterTransport::Network);
                let host =
                    discover_host_for_add(arguments.config.as_deref(), &add, non_interactive)
                        .await?;
                add.host = Some(host.address.to_string());
                add.port = Some(host.port);
            }
            add_printer(arguments.config.as_deref(), add, non_interactive)
        }
        PrintersCommand::Discover(discover) => {
            let path = configuration::resolved_path(arguments.config.as_deref())?;
            // Unlike `list`, a scan does not require a saved configuration to
            // already exist: an explicit --config naming a not-yet-created
            // file (the common case on a machine's first discovery run) is
            // not an error, only invalid TOML in an existing file is.
            let configuration = configuration::load_for_update(arguments.config.as_deref())?;
            eprintln!(
                "Reading configuration from {}",
                configuration::display_path(&path)
            );
            run_discover(discover, &configuration).await
        }
        #[cfg(target_os = "linux")]
        PrintersCommand::GrantUsbPermissions(args) => {
            grant_usb_permissions::run(args, non_interactive)
        }
    }
}
fn add_printer(
    config_path: Option<&std::path::Path>,
    arguments: AddPrinterArgs,
    non_interactive: bool,
) -> Result<(), CliError> {
    let can_prompt = !non_interactive && io::stdin().is_terminal() && io::stderr().is_terminal();
    execute_add(
        config_path,
        arguments,
        can_prompt,
        &mut InquireAddPrompter,
        &mut NusbInventory,
    )?;
    Ok(())
}
pub(crate) fn add_interactively(config_path: Option<&std::path::Path>) -> Result<String, CliError> {
    execute_add(
        config_path,
        AddPrinterArgs {
            name: None,
            transport: None,
            host: None,
            port: None,
            vendor_id: None,
            product_id: None,
            serial: None,
            profile: None,
            discover: false,
            subnet: Vec::new(),
            timeout: None,
        },
        true,
        &mut InquireAddPrompter,
        &mut NusbInventory,
    )
}
