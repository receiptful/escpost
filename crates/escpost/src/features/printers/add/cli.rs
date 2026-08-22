//! Registration: resolve `printers add`'s arguments (explicit flags,
//! interactive prompts, or USB/network discovery) into a saved printer.

use std::fmt;
use std::io::{self, IsTerminal};
use std::time::Duration;

use crate::application::ApplicationError;
use crate::configuration::{self, PrinterConfiguration};
use crate::error::CliError;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use inquire::validator::Validation;
use inquire::{CustomType, Select, Text};

use super::super::cli::output::{format_network_endpoint, usb_printer_label_parts};
use super::super::cli::scan_announcement;
use super::super::cli::{AddPrinterArgs, PrinterTransport};
use super::super::discover::cli::skipped_line;
use super::super::discover::{
    DiscoveryEvent, DiscoveryScope, NetworkDiscovery, NetworkScan, execute as execute_discovery,
    prepare as prepare_discovery,
};
use super::super::inventory::{NusbInventory, UsbInventory, UsbPrinter, configuration_matches};
use super::{AMBIGUOUS_USB_WARNING, Connection, DEFAULT_RAW_PORT, Request, Response, execute};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UsbAddTarget {
    vendor_id: u16,
    product_id: u16,
    bus: String,
    address: u8,
    manufacturer: Option<String>,
    product: Option<String>,
    serial_number: Option<String>,
    interface_number: u8,
    out_endpoint: u8,
    in_endpoint: Option<u8>,
    ambiguous_without_serial: bool,
}
/// A non-interactive request to register one connected USB printer by its
/// stable descriptor identity rather than by choosing it from a menu.
struct UsbSelector {
    vendor_id: u16,
    product_id: u16,
    serial: Option<String>,
}
#[derive(Debug, PartialEq, Eq)]
struct ResolvedAddPrinter {
    name: String,
    connection: ResolvedAddConnection,
    profile: Option<String>,
}
#[derive(Debug, PartialEq, Eq)]
enum ResolvedAddConnection {
    Usb(UsbAddTarget),
    Network { host: String, port: u16 },
}
pub(super) trait AddPrompter {
    fn name(&mut self) -> Result<String, CliError>;
    fn reject_name(&mut self, error: &CliError) {
        eprintln!("Error: {error}. Choose another printer name.");
    }
    fn transport(&mut self) -> Result<PrinterTransport, CliError>;
    fn usb_printer(
        &mut self,
        configured_name: Option<&str>,
        printers: Vec<UsbAddTarget>,
    ) -> Result<UsbAddTarget, CliError>;
    fn host(&mut self) -> Result<String, CliError>;
    fn port(&mut self) -> Result<u16, CliError>;
    fn profile(&mut self) -> Result<Option<String>, CliError>;
}
pub(crate) async fn run(
    config_path: Option<&std::path::Path>,
    mut arguments: AddPrinterArgs,
    non_interactive: bool,
) -> Result<String, CliError> {
    if arguments.discover {
        if arguments.transport == Some(PrinterTransport::Usb) {
            return Err(CliError::DiscoverForUsbPrinter);
        }
        arguments.transport = Some(PrinterTransport::Network);
        let printer = discover_printer_for_add(config_path, &arguments, non_interactive).await?;
        arguments.host = Some(printer.host);
        arguments.port = Some(printer.port);
    }
    let can_prompt = !non_interactive && io::stdin().is_terminal() && io::stderr().is_terminal();
    execute_add(
        config_path,
        arguments,
        can_prompt,
        &mut InquireAddPrompter,
        &mut super::super::inventory::NusbInventory,
    )
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
        &mut super::super::inventory::NusbInventory,
    )
}

fn execute_add(
    config_path: Option<&std::path::Path>,
    arguments: AddPrinterArgs,
    can_prompt: bool,
    prompter: &mut impl AddPrompter,
    inventory: &mut impl UsbInventory,
) -> Result<String, CliError> {
    let configuration = configuration::load_for_update(config_path)?;
    let resolved = resolve_add(arguments, can_prompt, prompter, inventory, &configuration)?;
    save_and_report_printer(config_path, &resolved)
}
fn save_and_report_printer(
    config_path: Option<&std::path::Path>,
    printer: &ResolvedAddPrinter,
) -> Result<String, CliError> {
    let ambiguous_without_serial = matches!(
        &printer.connection,
        ResolvedAddConnection::Usb(target) if target.ambiguous_without_serial
    );
    let connection = match &printer.connection {
        ResolvedAddConnection::Network { host, port } => Connection::Network {
            host: host.clone(),
            port: *port,
        },
        ResolvedAddConnection::Usb(target) => Connection::Usb {
            vendor_id: target.vendor_id,
            product_id: target.product_id,
            serial_number: target.serial_number.clone(),
            interface_number: target.interface_number,
            out_endpoint: target.out_endpoint,
            in_endpoint: target.in_endpoint,
        },
    };
    let response: Response = execute(Request::new(
        config_path.map(std::path::Path::to_owned),
        printer.name.clone(),
        printer.profile.clone(),
        connection,
    )?)?;
    eprintln!("Printer: {}", response.printer_name);
    eprintln!("Transport: {}", response.connection.transport());
    eprintln!(
        "Updated configuration at {}",
        response.config_path.display()
    );
    if ambiguous_without_serial {
        eprintln!("Warning: {AMBIGUOUS_USB_WARNING}");
    }
    Ok(response.printer_name)
}
fn resolve_add(
    arguments: AddPrinterArgs,
    can_prompt: bool,
    prompter: &mut impl AddPrompter,
    inventory: &mut impl UsbInventory,
    configuration: &PrinterConfiguration,
) -> Result<ResolvedAddPrinter, CliError> {
    let AddPrinterArgs {
        name,
        transport,
        host,
        port,
        vendor_id,
        product_id,
        serial,
        profile,
        // Already resolved to --host/--port by the Discover arm of `run`
        // before this function is reached.
        discover: _,
        subnet: _,
        timeout: _,
    } = arguments;
    if !can_prompt && name.is_none() {
        return Err(CliError::MissingPrinterName);
    }
    let interactive_wizard =
        can_prompt && (name.is_none() || transport.is_none() || host.is_none() || port.is_none());
    let transport = match transport {
        Some(transport) => transport,
        None if can_prompt => prompter.transport()?,
        None => return Err(CliError::MissingPrinterTransport),
    };
    let connection = match transport {
        PrinterTransport::Usb => {
            if host.is_some() {
                return Err(CliError::NetworkHostForUsbPrinter);
            }
            if port.is_some() {
                return Err(CliError::NetworkPortForUsbPrinter);
            }
            let selector = usb_selector(vendor_id, product_id, serial)?;
            // Without selectors, choosing a device and endpoint is a deliberate
            // act that only a terminal can perform.
            if !can_prompt && selector.is_none() {
                return Err(CliError::UsbRegistrationRequiresInteractive);
            }
            let candidates = usb_add_targets(inventory.list()?, configuration);
            ResolvedAddConnection::Usb(select_usb_target(
                candidates,
                selector.as_ref(),
                name.as_deref(),
                can_prompt,
                prompter,
            )?)
        }
        PrinterTransport::Network => {
            if vendor_id.is_some() || product_id.is_some() || serial.is_some() {
                return Err(CliError::UsbSelectorForNetworkPrinter);
            }
            let host = match host {
                Some(host) => host,
                None if can_prompt => prompter.host()?,
                None => return Err(CliError::MissingPrinterHost),
            };
            if host.trim().is_empty() {
                return Err(ApplicationError::BlankPrinterHost.into());
            }
            let port = match port {
                Some(port) => port,
                None if can_prompt => prompter.port()?,
                None => DEFAULT_RAW_PORT,
            };
            if port == 0 {
                return Err(ApplicationError::InvalidPrinterPort.into());
            }
            ResolvedAddConnection::Network { host, port }
        }
    };
    let name = resolve_name(name, can_prompt, prompter, configuration)?;
    // `interactive_wizard` is already true for every interactive USB add, so it
    // covers the profile prompt without letting a non-interactive USB add try to
    // read from a terminal that is not there.
    let profile = match profile {
        Some(profile) => Some(profile),
        None if interactive_wizard => prompter.profile()?,
        None => None,
    };
    if profile
        .as_deref()
        .is_some_and(|profile| profile.trim().is_empty())
    {
        return Err(ApplicationError::BlankPrinterProfile.into());
    }

    Ok(ResolvedAddPrinter {
        name,
        connection,
        profile,
    })
}
/// Build a USB selector from the descriptor options. Vendor and product IDs
/// identify a model together, so neither is meaningful alone; a serial number
/// only further narrows that identity.
fn usb_selector(
    vendor_id: Option<u16>,
    product_id: Option<u16>,
    serial: Option<String>,
) -> Result<Option<UsbSelector>, CliError> {
    match (vendor_id, product_id) {
        (Some(vendor_id), Some(product_id)) => Ok(Some(UsbSelector {
            vendor_id,
            product_id,
            serial,
        })),
        (None, None) if serial.is_none() => Ok(None),
        _ => Err(CliError::IncompleteUsbSelector),
    }
}
/// Resolve the connected USB route to register. Without a selector this is an
/// interactive menu; with one the descriptor must identify exactly one route,
/// and a still-ambiguous choice of endpoint is deferred to the terminal rather
/// than guessed.
fn select_usb_target(
    candidates: Vec<UsbAddTarget>,
    selector: Option<&UsbSelector>,
    configured_name: Option<&str>,
    can_prompt: bool,
    prompter: &mut impl AddPrompter,
) -> Result<UsbAddTarget, CliError> {
    let Some(selector) = selector else {
        if candidates.is_empty() {
            return Err(CliError::NoUnconfiguredUsbPrinters);
        }
        return prompter.usb_printer(configured_name, candidates);
    };

    let mut matched = filter_usb_targets(candidates, selector);
    match matched.len() {
        0 => Err(CliError::NoMatchingUsbPrinter),
        1 => Ok(matched.remove(0)),
        _ if can_prompt => prompter.usb_printer(configured_name, matched),
        _ => Err(CliError::AmbiguousUsbPrinter),
    }
}
/// Keep only the unconfigured routes whose stable descriptor matches the
/// selector. An omitted serial matches any device of the requested model.
fn filter_usb_targets(targets: Vec<UsbAddTarget>, selector: &UsbSelector) -> Vec<UsbAddTarget> {
    targets
        .into_iter()
        .filter(|target| {
            target.vendor_id == selector.vendor_id
                && target.product_id == selector.product_id
                && selector
                    .serial
                    .as_deref()
                    .is_none_or(|serial| target.serial_number.as_deref() == Some(serial))
        })
        .collect()
}
fn resolve_name(
    explicit_name: Option<String>,
    can_prompt: bool,
    prompter: &mut impl AddPrompter,
    configuration: &PrinterConfiguration,
) -> Result<String, CliError> {
    if !can_prompt {
        let name = explicit_name.ok_or(CliError::MissingPrinterName)?;
        validate_name(&name, configuration)?;
        return Ok(name);
    }

    let mut candidate = explicit_name;
    loop {
        let name = match candidate.take() {
            Some(name) => name,
            None => prompter.name()?,
        };
        match validate_name(&name, configuration) {
            Ok(()) => return Ok(name),
            Err(error) => prompter.reject_name(&error),
        }
    }
}
fn validate_name(name: &str, configuration: &PrinterConfiguration) -> Result<(), CliError> {
    if name.trim().is_empty() {
        return Err(ApplicationError::BlankPrinterName.into());
    }
    if configuration.printer(name).is_some() {
        return Err(ApplicationError::PrinterAlreadyConfigured(name.to_owned()).into());
    }
    Ok(())
}
struct InquireAddPrompter;

impl AddPrompter for InquireAddPrompter {
    fn name(&mut self) -> Result<String, CliError> {
        Text::new("Printer name")
            .prompt()
            .map_err(|error| CliError::PrinterPrompt(error.to_string()))
    }

    fn transport(&mut self) -> Result<PrinterTransport, CliError> {
        Select::new(
            "Transport",
            vec![PrinterTransport::Usb, PrinterTransport::Network],
        )
        .prompt()
        .map_err(|error| CliError::PrinterPrompt(error.to_string()))
    }

    fn usb_printer(
        &mut self,
        configured_name: Option<&str>,
        printers: Vec<UsbAddTarget>,
    ) -> Result<UsbAddTarget, CliError> {
        Select::new(&usb_printer_prompt(configured_name), printers)
            .prompt()
            .map_err(|error| CliError::PrinterPrompt(error.to_string()))
    }

    fn host(&mut self) -> Result<String, CliError> {
        Text::new("Network host")
            .prompt()
            .map_err(|error| CliError::PrinterPrompt(error.to_string()))
    }

    fn port(&mut self) -> Result<u16, CliError> {
        CustomType::<u16>::new("Network port")
            .with_default(DEFAULT_RAW_PORT)
            .with_error_message("Enter a port between 1 and 65535")
            .with_validator(|port: &u16| {
                Ok(if *port == 0 {
                    Validation::Invalid("Port must be between 1 and 65535".into())
                } else {
                    Validation::Valid
                })
            })
            .prompt()
            .map_err(|error| CliError::PrinterPrompt(error.to_string()))
    }

    fn profile(&mut self) -> Result<Option<String>, CliError> {
        let profile = Text::new("Printer profile (optional)")
            .with_help_message("Leave empty when the printer has not been calibrated yet")
            .prompt()
            .map_err(|error| CliError::PrinterPrompt(error.to_string()))?;
        Ok((!profile.trim().is_empty()).then(|| profile.trim().to_owned()))
    }
}

fn usb_printer_prompt(configured_name: Option<&str>) -> String {
    match configured_name {
        Some(configured_name) => {
            format!("Select the USB device to register as {configured_name:?}")
        }
        None => "Select the USB device to register".to_owned(),
    }
}
impl fmt::Display for PrinterTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usb => formatter.write_str("usb"),
            Self::Network => formatter.write_str("network"),
        }
    }
}
impl fmt::Display for UsbAddTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let model = usb_printer_label_parts(self.product.as_deref(), self.manufacturer.as_deref());
        write!(
            formatter,
            "{model} ({:04x}:{:04x};",
            self.vendor_id, self.product_id
        )?;
        if let Some(serial_number) = &self.serial_number {
            write!(formatter, " serial {serial_number};")?;
        } else {
            formatter.write_str(" no serial;")?;
        }
        write!(
            formatter,
            " bus {} address {}; interface {}; OUT {:#04x})",
            self.bus, self.address, self.interface_number, self.out_endpoint
        )
    }
}
/// A discovered endpoint offered for registration, labeled with any saved
/// printers already pointing at it.
#[derive(Debug)]
struct DiscoverChoice {
    printer: NetworkDiscovery,
}

impl fmt::Display for DiscoverChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}",
            format_network_endpoint(&self.printer.host, self.printer.port)
        )?;
        let mut notes = Vec::new();
        if let Some(interface) = &self.printer.interface {
            notes.push(format!("via {interface}"));
        }
        if !self.printer.configured_names.is_empty() {
            notes.push(format!(
                "configured as {}",
                self.printer.configured_names.join(", ")
            ));
        }
        if !notes.is_empty() {
            write!(formatter, " ({})", notes.join("; "))?;
        }
        Ok(())
    }
}
trait DiscoverPicker {
    fn discovered_host(&mut self, choices: Vec<DiscoverChoice>)
    -> Result<DiscoverChoice, CliError>;
}
struct InquireDiscoverPicker;

impl DiscoverPicker for InquireDiscoverPicker {
    fn discovered_host(
        &mut self,
        choices: Vec<DiscoverChoice>,
    ) -> Result<DiscoverChoice, CliError> {
        Select::new("Network printer", choices)
            .prompt()
            .map_err(|error| CliError::PrinterPrompt(error.to_string()))
    }
}
async fn discover_printer_for_add(
    config_path: Option<&std::path::Path>,
    arguments: &AddPrinterArgs,
    non_interactive: bool,
) -> Result<NetworkDiscovery, CliError> {
    let port = arguments.port.unwrap_or(9100);
    let scope = DiscoveryScope::Network(NetworkScan::new(
        port,
        arguments.subnet.clone(),
        Duration::from_millis(arguments.timeout.unwrap_or(1000)),
    )?);
    let prepared = prepare_discovery(config_path.map(std::path::Path::to_owned), scope)?;
    let bar = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::stderr());
    bar.set_style(
        ProgressStyle::with_template("{msg} [{bar:40}] {pos}/{len}")
            .expect("the progress bar template is a compile-time constant")
            .progress_chars("=> "),
    );
    bar.set_message("Scanning for network printers");
    let mut length_set = false;
    let response = execute_discovery(
        prepared,
        |event| match event {
            DiscoveryEvent::Prepared {
                scope,
                scan_targets,
                skipped,
                ..
            } => {
                let scan = scope
                    .network_scan()
                    .expect("add discovery always prepares a network scope");
                // The same scan as `printers discover`, so the same
                // omissions are reported, in the same words: an adapter left
                // out silently reads as a network that holds no printer.
                for adapter in skipped {
                    eprintln!("{}", skipped_line(adapter));
                }
                eprintln!("{}", scan_announcement(scan_targets, scan.port()));
                if scan.uses_automatic_subnets() {
                    eprintln!("Tip: pass --subnet <CIDR> to scan a different network.");
                }
            }
            // Discovery for `add` is always network-only, so USB events
            // never fire here; the final `Response` is what selection reads.
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
    let can_prompt = !non_interactive && io::stdin().is_terminal() && io::stderr().is_terminal();
    choose_discovered_printer(
        response.network_printers,
        port,
        can_prompt,
        &mut InquireDiscoverPicker,
    )
}
/// Resolve the sweep result to one endpoint. Exactly one candidate needs no
/// prompt; several candidates need a terminal, because choosing a printer
/// implicitly could register a stranger's device.
fn choose_discovered_printer(
    printers: Vec<NetworkDiscovery>,
    port: u16,
    can_prompt: bool,
    picker: &mut impl DiscoverPicker,
) -> Result<NetworkDiscovery, CliError> {
    let mut choices = printers
        .into_iter()
        .map(|printer| DiscoverChoice { printer })
        .collect::<Vec<_>>();
    match choices.len() {
        0 => Err(CliError::NoDiscoveredPrinters(port)),
        1 => Ok(choices.remove(0).printer),
        _ if can_prompt => Ok(picker.discovered_host(choices)?.printer),
        _ => Err(CliError::AmbiguousDiscoveredPrinters(
            choices.iter().map(ToString::to_string).collect(),
        )),
    }
}
fn usb_add_targets(
    printers: Vec<UsbPrinter>,
    configuration: &PrinterConfiguration,
) -> Vec<UsbAddTarget> {
    let unconfigured = printers
        .into_iter()
        .filter(|printer| {
            !configuration
                .usb_printers()
                .iter()
                .any(|configured| configuration_matches(printer, configured))
        })
        .collect::<Vec<_>>();
    let mut targets = Vec::new();

    for printer in &unconfigured {
        // Bus and address are useful for distinguishing devices in this
        // one-time menu, but the operating system may assign new values after
        // reconnecting. The saved identity therefore uses stable descriptors.
        let ambiguous_without_serial = printer.serial_number.is_none()
            && unconfigured.iter().any(|other| {
                other.vendor_id == printer.vendor_id
                    && other.product_id == printer.product_id
                    && (other.bus != printer.bus || other.address != printer.address)
            });
        let in_endpoint = (printer.in_endpoints.len() == 1).then(|| printer.in_endpoints[0]);

        // Most printers expose one bulk OUT endpoint. If firmware exposes
        // several, present each as a separate explicit choice rather than
        // silently choosing a route that may not carry print data.
        for out_endpoint in &printer.out_endpoints {
            targets.push(UsbAddTarget {
                vendor_id: printer.vendor_id,
                product_id: printer.product_id,
                bus: printer.bus.clone(),
                address: printer.address,
                manufacturer: printer.manufacturer.clone(),
                product: printer.product.clone(),
                serial_number: printer.serial_number.clone(),
                interface_number: printer.interface_number,
                out_endpoint: *out_endpoint,
                in_endpoint,
                ambiguous_without_serial,
            });
        }
    }

    targets.sort_by_cached_key(|target| {
        let label =
            usb_printer_label_parts(target.product.as_deref(), target.manufacturer.as_deref());
        (
            label.to_lowercase(),
            label,
            target.bus.clone(),
            target.address,
            target.interface_number,
            target.out_endpoint,
        )
    });
    targets
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::net::TcpListener;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::super::test_support::{FixedInventory, discovered, netum_usb_printer};
    use super::*;
    use crate::configuration::PrinterConfiguration;
    use crate::discovery::Subnet;
    use crate::features::printers::discover::NetworkDiscovery;

    #[test]
    fn usb_selection_prompt_names_the_configured_alias_when_present() {
        assert_eq!(
            usb_printer_prompt(Some("nt5890k-usb")),
            "Select the USB device to register as \"nt5890k-usb\""
        );
    }

    #[test]
    fn usb_selection_prompt_is_neutral_before_an_alias_is_chosen() {
        assert_eq!(
            usb_printer_prompt(None),
            "Select the USB device to register"
        );
    }

    #[test]
    fn interactive_network_add_prompts_for_the_port() {
        let arguments = AddPrinterArgs {
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
        };
        let mut prompter = FixedAddPrompter::with_names(["kitchen"]);

        let resolved = resolve_add(
            arguments,
            true,
            &mut prompter,
            &mut FixedInventory {
                printers: Vec::new(),
            },
            &PrinterConfiguration::default(),
        )
        .expect("interactive values should resolve");

        assert_eq!(
            resolved,
            ResolvedAddPrinter {
                name: "kitchen".to_owned(),
                connection: ResolvedAddConnection::Network {
                    host: "10.42.0.71".to_owned(),
                    port: 9200,
                },
                profile: Some("REFERENCE".to_owned()),
            }
        );
        assert_eq!(prompter.port_prompts, 1);
    }

    #[test]
    fn explicit_network_port_skips_port_and_profile_prompts() {
        let arguments = AddPrinterArgs {
            name: Some("kitchen".to_owned()),
            transport: Some(PrinterTransport::Network),
            host: Some("10.42.0.71".to_owned()),
            port: Some(9100),
            vendor_id: None,
            product_id: None,
            serial: None,
            profile: None,
            discover: false,
            subnet: Vec::new(),
            timeout: None,
        };

        let resolved = resolve_add(
            arguments,
            true,
            &mut UnexpectedAddPrompter,
            &mut FixedInventory {
                printers: Vec::new(),
            },
            &PrinterConfiguration::default(),
        )
        .expect("complete explicit values should resolve");

        assert_eq!(resolved.profile, None);
    }

    #[test]
    fn usb_add_rejects_an_explicit_network_port() {
        let arguments = AddPrinterArgs {
            name: Some("counter".to_owned()),
            transport: Some(PrinterTransport::Usb),
            host: None,
            port: Some(9100),
            vendor_id: None,
            product_id: None,
            serial: None,
            profile: None,
            discover: false,
            subnet: Vec::new(),
            timeout: None,
        };

        let error = resolve_add(
            arguments,
            true,
            &mut UnexpectedAddPrompter,
            &mut FixedInventory {
                printers: Vec::new(),
            },
            &PrinterConfiguration::default(),
        )
        .expect_err("a USB configuration must not accept network coordinates");

        assert!(matches!(error, CliError::NetworkPortForUsbPrinter));
    }

    #[test]
    fn interactive_add_reprompts_when_its_explicit_name_already_exists() {
        let configuration = PrinterConfiguration::parse(
            r#"
[kitchen]
transport = "network"
host = "10.42.0.20"
port = 9100
"#,
        )
        .expect("the existing printer should parse");
        let arguments = AddPrinterArgs {
            name: Some("kitchen".to_owned()),
            transport: Some(PrinterTransport::Network),
            host: Some("10.42.0.71".to_owned()),
            port: Some(9100),
            vendor_id: None,
            product_id: None,
            serial: None,
            profile: Some("REFERENCE".to_owned()),
            discover: false,
            subnet: Vec::new(),
            timeout: None,
        };
        let mut prompter = FixedAddPrompter::with_names(["counter"]);

        let resolved = resolve_add(
            arguments,
            true,
            &mut prompter,
            &mut FixedInventory {
                printers: Vec::new(),
            },
            &configuration,
        )
        .expect("a second unique name should continue registration");

        assert_eq!(resolved.name, "counter");
        assert_eq!(
            prompter.rejected_names,
            vec!["printer \"kitchen\" is already configured"]
        );
        assert_eq!(prompter.port_prompts, 0);
    }

    #[test]
    fn interactive_usb_add_saves_the_selected_descriptor_coordinates() {
        let directory = temporary_directory("add-usb");
        let configuration = directory.join("printers.toml");
        let arguments = AddPrinterArgs {
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
        };
        let mut inventory = FixedInventory {
            printers: vec![UsbPrinter {
                vendor_id: 0x0416,
                product_id: 0x5011,
                bus: "003".to_owned(),
                address: 60,
                manufacturer: Some("YICHIP3121".to_owned()),
                product: Some("USB Portable Printer".to_owned()),
                serial_number: Some("B120300001".to_owned()),
                interface_number: 0,
                out_endpoints: vec![0x01],
                in_endpoints: vec![0x81],
            }],
        };

        let name = execute_add(
            Some(&configuration),
            arguments,
            true,
            &mut UsbAddPrompter {
                expected_configured_name: None,
            },
            &mut inventory,
        )
        .expect("the selected USB printer should be saved");

        assert_eq!(name, "counter-usb");
        let document = fs::read_to_string(&configuration)
            .expect("the printer configuration should be readable");
        let table =
            toml::from_str::<toml::Table>(&document).expect("the configuration should be TOML");
        let printer = table["counter-usb"]
            .as_table()
            .expect("the configured printer should be a table");
        assert_eq!(printer["transport"].as_str(), Some("usb"));
        assert_eq!(printer["profile"].as_str(), Some("REFERENCE"));
        assert_eq!(printer["vendor_id"].as_str(), Some("0x0416"));
        assert_eq!(printer["product_id"].as_str(), Some("0x5011"));
        assert_eq!(printer["serial_number"].as_str(), Some("B120300001"));
        assert_eq!(printer["interface_number"].as_integer(), Some(0));
        assert_eq!(printer["out_endpoint"].as_str(), Some("0x01"));
        assert_eq!(printer["in_endpoint"].as_str(), Some("0x81"));
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[test]
    fn explicit_name_is_available_while_selecting_a_usb_device() {
        let arguments = AddPrinterArgs {
            name: Some("nt5890k-usb".to_owned()),
            transport: Some(PrinterTransport::Usb),
            host: None,
            port: None,
            vendor_id: None,
            product_id: None,
            serial: None,
            profile: Some("REFERENCE".to_owned()),
            discover: false,
            subnet: Vec::new(),
            timeout: None,
        };
        let mut inventory = FixedInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
        };

        let resolved = resolve_add(
            arguments,
            true,
            &mut UsbAddPrompter {
                expected_configured_name: Some("nt5890k-usb"),
            },
            &mut inventory,
            &PrinterConfiguration::default(),
        )
        .expect("the explicit name should remain available during USB selection");

        assert_eq!(resolved.name, "nt5890k-usb");
    }

    #[test]
    fn configured_usb_printers_are_not_offered_for_addition() {
        let configuration = PrinterConfiguration::parse(
            r#"
[counter]
transport = "usb"
vendor_id = "0x0416"
product_id = "0x5011"
serial_number = "B120300001"
interface_number = 0
out_endpoint = "0x01"
"#,
        )
        .expect("the saved printer should parse");

        let targets = usb_add_targets(
            vec![netum_usb_printer(vec![0x01], vec![0x81])],
            &configuration,
        );

        assert!(
            targets.is_empty(),
            "a connected printer already represented by the configuration must not be offered again"
        );
    }

    #[test]
    fn every_bulk_out_endpoint_is_an_explicit_usb_add_choice() {
        let targets = usb_add_targets(
            vec![netum_usb_printer(vec![0x01, 0x02], vec![0x81, 0x82])],
            &PrinterConfiguration::default(),
        );

        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].out_endpoint, 0x01);
        assert_eq!(targets[1].out_endpoint, 0x02);
        assert_eq!(
            targets[0].in_endpoint, None,
            "several IN endpoints must not be reduced to an arbitrary guess"
        );
        assert_eq!(targets[1].in_endpoint, None);
    }

    #[test]
    fn identical_usb_devices_without_serials_are_marked_ambiguous() {
        let mut first = netum_usb_printer(vec![0x01], vec![0x81]);
        first.serial_number = None;
        let mut second = first.clone();
        second.address = 61;

        let targets = usb_add_targets(vec![first, second], &PrinterConfiguration::default());

        assert_eq!(targets.len(), 2);
        assert!(
            targets.iter().all(|target| target.ambiguous_without_serial),
            "both saved identities would match both connected physical devices"
        );
    }

    #[test]
    fn usb_add_choice_explains_the_descriptor_and_route_being_saved() {
        let target = usb_add_targets(
            vec![netum_usb_printer(vec![0x01], vec![0x81])],
            &PrinterConfiguration::default(),
        )
        .remove(0);

        assert_eq!(
            target.to_string(),
            "USB Portable Printer (YICHIP3121) (0416:5011; serial B120300001; bus 003 address 60; interface 0; OUT 0x01)"
        );
    }

    #[test]
    fn usb_selector_requires_both_vendor_and_product() {
        assert!(
            usb_selector(None, None, None)
                .expect("no selector is valid")
                .is_none()
        );
        assert!(matches!(
            usb_selector(Some(0x0416), None, None),
            Err(CliError::IncompleteUsbSelector)
        ));
        assert!(matches!(
            usb_selector(None, Some(0x5011), None),
            Err(CliError::IncompleteUsbSelector)
        ));
        assert!(matches!(
            usb_selector(None, None, Some("B120300001".to_owned())),
            Err(CliError::IncompleteUsbSelector)
        ));
    }

    #[test]
    fn a_serial_selector_narrows_identical_usb_models() {
        let mut first = netum_usb_printer(vec![0x01], vec![0x81]);
        first.serial_number = Some("FIRST".to_owned());
        let mut second = netum_usb_printer(vec![0x01], vec![0x81]);
        second.serial_number = Some("SECOND".to_owned());
        second.address = 61;
        let targets = usb_add_targets(vec![first, second], &PrinterConfiguration::default());

        let matched = filter_usb_targets(
            targets,
            &UsbSelector {
                vendor_id: 0x0416,
                product_id: 0x5011,
                serial: Some("SECOND".to_owned()),
            },
        );

        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].serial_number.as_deref(), Some("SECOND"));
    }

    #[test]
    fn a_non_interactive_selector_uses_a_unique_match_without_prompting() {
        let targets = usb_add_targets(
            vec![netum_usb_printer(vec![0x01], vec![0x81])],
            &PrinterConfiguration::default(),
        );

        let target = select_usb_target(
            targets,
            Some(&UsbSelector {
                vendor_id: 0x0416,
                product_id: 0x5011,
                serial: None,
            }),
            None,
            false,
            &mut UnexpectedAddPrompter,
        )
        .expect("a unique descriptor match should resolve without a menu");

        assert_eq!(target.vendor_id, 0x0416);
        assert_eq!(target.out_endpoint, 0x01);
    }

    #[test]
    fn a_non_interactive_selector_that_matches_nothing_is_an_error() {
        let targets = usb_add_targets(
            vec![netum_usb_printer(vec![0x01], vec![0x81])],
            &PrinterConfiguration::default(),
        );

        let error = select_usb_target(
            targets,
            Some(&UsbSelector {
                vendor_id: 0x1234,
                product_id: 0x5678,
                serial: None,
            }),
            None,
            false,
            &mut UnexpectedAddPrompter,
        )
        .expect_err("an unmatched selector must not save anything");

        assert!(matches!(error, CliError::NoMatchingUsbPrinter));
    }

    #[test]
    fn a_non_interactive_ambiguous_selector_refuses_to_guess() {
        let targets = usb_add_targets(
            vec![netum_usb_printer(vec![0x01, 0x02], vec![0x81])],
            &PrinterConfiguration::default(),
        );

        let error = select_usb_target(
            targets,
            Some(&UsbSelector {
                vendor_id: 0x0416,
                product_id: 0x5011,
                serial: None,
            }),
            None,
            false,
            &mut UnexpectedAddPrompter,
        )
        .expect_err("two bulk OUT endpoints must not be silently reduced to one");

        assert!(matches!(error, CliError::AmbiguousUsbPrinter));
    }

    #[test]
    fn an_interactive_ambiguous_selector_defers_the_endpoint_choice() {
        let targets = usb_add_targets(
            vec![netum_usb_printer(vec![0x01, 0x02], vec![0x81])],
            &PrinterConfiguration::default(),
        );

        let target = select_usb_target(
            targets,
            Some(&UsbSelector {
                vendor_id: 0x0416,
                product_id: 0x5011,
                serial: None,
            }),
            None,
            true,
            &mut FirstUsbPrompter,
        )
        .expect("a terminal can still pick among the narrowed routes");

        assert_eq!(target.out_endpoint, 0x01);
    }

    #[test]
    fn non_interactive_usb_add_saves_the_selected_descriptor_coordinates() {
        let directory = temporary_directory("non-interactive-add-usb");
        let configuration = directory.join("printers.toml");
        let arguments = AddPrinterArgs {
            name: Some("counter-usb".to_owned()),
            transport: Some(PrinterTransport::Usb),
            host: None,
            port: None,
            vendor_id: Some(0x0416),
            product_id: Some(0x5011),
            serial: Some("B120300001".to_owned()),
            profile: Some("NT-5890K".to_owned()),
            discover: false,
            subnet: Vec::new(),
            timeout: None,
        };
        let mut inventory = FixedInventory {
            printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
        };

        let name = execute_add(
            Some(&configuration),
            arguments,
            false,
            &mut UnexpectedAddPrompter,
            &mut inventory,
        )
        .expect("a matched USB printer should be saved without prompting");

        assert_eq!(name, "counter-usb");
        let document = fs::read_to_string(&configuration)
            .expect("the printer configuration should be readable");
        let table =
            toml::from_str::<toml::Table>(&document).expect("the configuration should be TOML");
        let printer = table["counter-usb"]
            .as_table()
            .expect("the configured printer should be a table");
        assert_eq!(printer["transport"].as_str(), Some("usb"));
        assert_eq!(printer["profile"].as_str(), Some("NT-5890K"));
        assert_eq!(printer["vendor_id"].as_str(), Some("0x0416"));
        assert_eq!(printer["product_id"].as_str(), Some("0x5011"));
        assert_eq!(printer["serial_number"].as_str(), Some("B120300001"));
        assert_eq!(printer["out_endpoint"].as_str(), Some("0x01"));
        assert_eq!(printer["in_endpoint"].as_str(), Some("0x81"));
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[test]
    fn non_interactive_usb_add_without_selectors_requires_a_terminal() {
        let arguments = AddPrinterArgs {
            name: Some("counter-usb".to_owned()),
            transport: Some(PrinterTransport::Usb),
            host: None,
            port: None,
            vendor_id: None,
            product_id: None,
            serial: None,
            profile: None,
            discover: false,
            subnet: Vec::new(),
            timeout: None,
        };

        let error = resolve_add(
            arguments,
            false,
            &mut UnexpectedAddPrompter,
            &mut FixedInventory {
                printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
            },
            &PrinterConfiguration::default(),
        )
        .expect_err("choosing a device without a selector needs a terminal");

        assert!(matches!(
            error,
            CliError::UsbRegistrationRequiresInteractive
        ));
    }

    #[test]
    fn usb_selectors_are_rejected_for_a_network_printer() {
        let arguments = AddPrinterArgs {
            name: Some("kitchen".to_owned()),
            transport: Some(PrinterTransport::Network),
            host: Some("10.42.0.71".to_owned()),
            port: None,
            vendor_id: Some(0x0416),
            product_id: Some(0x5011),
            serial: None,
            profile: None,
            discover: false,
            subnet: Vec::new(),
            timeout: None,
        };

        let error = resolve_add(
            arguments,
            false,
            &mut UnexpectedAddPrompter,
            &mut FixedInventory {
                printers: Vec::new(),
            },
            &PrinterConfiguration::default(),
        )
        .expect_err("a network printer must not accept USB descriptors");

        assert!(matches!(error, CliError::UsbSelectorForNetworkPrinter));
    }

    struct FirstUsbPrompter;

    impl AddPrompter for FirstUsbPrompter {
        fn name(&mut self) -> Result<String, CliError> {
            panic!("name prompt was not expected")
        }

        fn transport(&mut self) -> Result<PrinterTransport, CliError> {
            panic!("transport prompt was not expected")
        }

        fn usb_printer(
            &mut self,
            _configured_name: Option<&str>,
            mut printers: Vec<UsbAddTarget>,
        ) -> Result<UsbAddTarget, CliError> {
            assert!(
                printers.len() > 1,
                "a unique match should not reach the menu"
            );
            Ok(printers.remove(0))
        }

        fn host(&mut self) -> Result<String, CliError> {
            panic!("host prompt was not expected")
        }

        fn port(&mut self) -> Result<u16, CliError> {
            panic!("port prompt was not expected")
        }

        fn profile(&mut self) -> Result<Option<String>, CliError> {
            panic!("profile prompt was not expected")
        }
    }

    struct FixedAddPrompter {
        names: VecDeque<String>,
        rejected_names: Vec<String>,
        port_prompts: usize,
    }

    struct UsbAddPrompter {
        expected_configured_name: Option<&'static str>,
    }

    struct UnexpectedAddPrompter;

    impl FixedAddPrompter {
        fn with_names<const N: usize>(names: [&str; N]) -> Self {
            Self {
                names: names.map(str::to_owned).into(),
                rejected_names: Vec::new(),
                port_prompts: 0,
            }
        }
    }

    impl AddPrompter for FixedAddPrompter {
        fn name(&mut self) -> Result<String, CliError> {
            Ok(self
                .names
                .pop_front()
                .expect("the resolver should not exhaust test names"))
        }

        fn reject_name(&mut self, error: &CliError) {
            self.rejected_names.push(error.to_string());
        }

        fn transport(&mut self) -> Result<PrinterTransport, CliError> {
            Ok(PrinterTransport::Network)
        }

        fn usb_printer(
            &mut self,
            _configured_name: Option<&str>,
            _printers: Vec<UsbAddTarget>,
        ) -> Result<UsbAddTarget, CliError> {
            panic!("a network printer must not ask for a USB device")
        }

        fn host(&mut self) -> Result<String, CliError> {
            Ok("10.42.0.71".to_owned())
        }

        fn port(&mut self) -> Result<u16, CliError> {
            self.port_prompts += 1;
            Ok(9200)
        }

        fn profile(&mut self) -> Result<Option<String>, CliError> {
            Ok(Some("REFERENCE".to_owned()))
        }
    }

    impl AddPrompter for UsbAddPrompter {
        fn name(&mut self) -> Result<String, CliError> {
            Ok("counter-usb".to_owned())
        }

        fn transport(&mut self) -> Result<PrinterTransport, CliError> {
            Ok(PrinterTransport::Usb)
        }

        fn usb_printer(
            &mut self,
            configured_name: Option<&str>,
            mut printers: Vec<UsbAddTarget>,
        ) -> Result<UsbAddTarget, CliError> {
            assert_eq!(configured_name, self.expected_configured_name);
            assert_eq!(printers.len(), 1);
            Ok(printers.remove(0))
        }

        fn host(&mut self) -> Result<String, CliError> {
            panic!("a USB printer must not ask for a network host")
        }

        fn port(&mut self) -> Result<u16, CliError> {
            panic!("a USB printer must not ask for a network port")
        }

        fn profile(&mut self) -> Result<Option<String>, CliError> {
            Ok(Some("REFERENCE".to_owned()))
        }
    }

    impl AddPrompter for UnexpectedAddPrompter {
        fn name(&mut self) -> Result<String, CliError> {
            panic!("name prompt was not expected")
        }

        fn transport(&mut self) -> Result<PrinterTransport, CliError> {
            panic!("transport prompt was not expected")
        }

        fn usb_printer(
            &mut self,
            _configured_name: Option<&str>,
            _printers: Vec<UsbAddTarget>,
        ) -> Result<UsbAddTarget, CliError> {
            panic!("USB printer prompt was not expected")
        }

        fn host(&mut self) -> Result<String, CliError> {
            panic!("host prompt was not expected")
        }

        fn port(&mut self) -> Result<u16, CliError> {
            panic!("port prompt was not expected")
        }

        fn profile(&mut self) -> Result<Option<String>, CliError> {
            panic!("profile prompt was not expected")
        }
    }

    fn temporary_directory(case: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock should be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "escpost-printers-{case}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("the test directory should be creatable");
        path
    }

    struct UnexpectedDiscoverPicker;

    impl DiscoverPicker for UnexpectedDiscoverPicker {
        fn discovered_host(
            &mut self,
            _choices: Vec<DiscoverChoice>,
        ) -> Result<DiscoverChoice, CliError> {
            panic!("no discovery selection prompt was expected");
        }
    }

    struct FirstChoiceDiscoverPicker;

    impl DiscoverPicker for FirstChoiceDiscoverPicker {
        fn discovered_host(
            &mut self,
            mut choices: Vec<DiscoverChoice>,
        ) -> Result<DiscoverChoice, CliError> {
            Ok(choices.remove(0))
        }
    }

    fn network_discovery(address: [u8; 4], port: u16) -> NetworkDiscovery {
        let host = discovered(address, port);
        NetworkDiscovery {
            configured_names: Vec::new(),
            configured_profile: None,
            host: host.address.to_string(),
            port: host.port,
            interface: host.interface,
        }
    }

    #[tokio::test]
    async fn add_discovery_uses_shared_correlation_before_concrete_add() {
        // 127.0.0.2 rather than 127.0.0.1: the whole 127.0.0.0/8 block routes
        // to loopback, but only 127.0.0.1 is the machine's own address, so an
        // explicit /32 on 127.0.0.2 is not self-excluded and this stand-in
        // "printer" stays discoverable.
        let listener = TcpListener::bind("127.0.0.2:0").expect("an ephemeral port should bind");
        let port = listener
            .local_addr()
            .expect("the listener should have an address")
            .port();
        let directory = temporary_directory("shared-discovery-add");
        let config = directory.join("printers.toml");
        fs::write(
            &config,
            format!("[existing]\ntransport = \"network\"\nhost = \"127.0.0.2\"\nport = {port}\n"),
        )
        .expect("the existing configuration should be writable");
        let mut arguments = AddPrinterArgs {
            name: Some("alias".to_owned()),
            transport: Some(PrinterTransport::Network),
            host: None,
            port: Some(port),
            vendor_id: None,
            product_id: None,
            serial: None,
            profile: None,
            discover: true,
            subnet: vec![Subnet::parse("127.0.0.2/32").expect("valid subnet")],
            timeout: Some(50),
        };

        let selected = discover_printer_for_add(Some(&config), &arguments, true)
            .await
            .expect("shared discovery should select the listener");
        assert_eq!(selected.configured_names, vec!["existing"]);
        arguments.host = Some(selected.host);
        arguments.port = Some(selected.port);
        arguments.discover = false;

        let added = execute_add(
            Some(&config),
            arguments,
            false,
            &mut UnexpectedAddPrompter,
            &mut FixedInventory {
                printers: Vec::new(),
            },
        )
        .expect("the concrete add should follow shared discovery");

        assert_eq!(added, "alias");
        let document = fs::read_to_string(&config).expect("the updated config should be readable");
        assert!(document.contains("[existing]"));
        assert!(document.contains("[alias]"));
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[test]
    fn a_single_discovered_host_is_chosen_without_prompting() {
        let printers = vec![network_discovery([10, 42, 0, 71], 9100)];

        let chosen =
            choose_discovered_printer(printers, 9100, false, &mut UnexpectedDiscoverPicker)
                .expect("one candidate needs no prompt");

        assert_eq!(chosen.host, "10.42.0.71");
        assert_eq!(chosen.port, 9100);
    }

    #[test]
    fn zero_discovered_hosts_is_an_error() {
        let error =
            choose_discovered_printer(Vec::new(), 9100, true, &mut UnexpectedDiscoverPicker)
                .expect_err("nothing to add must fail");

        assert!(matches!(error, CliError::NoDiscoveredPrinters(9100)));
    }

    #[test]
    fn several_discovered_hosts_without_a_terminal_is_an_error_naming_them() {
        let mut configured = network_discovery([10, 42, 0, 71], 9100);
        configured.configured_names = vec!["kitchen".to_owned()];
        let printers = vec![network_discovery([10, 42, 0, 5], 9100), configured];

        let error = choose_discovered_printer(printers, 9100, false, &mut UnexpectedDiscoverPicker)
            .expect_err("an implicit choice among several hosts must be refused");

        let CliError::AmbiguousDiscoveredPrinters(names) = error else {
            panic!("expected AmbiguousDiscoveredPrinters, got {error:?}");
        };
        assert_eq!(
            names,
            vec![
                "10.42.0.5:9100 (via enx0)".to_owned(),
                "10.42.0.71:9100 (via enx0; configured as kitchen)".to_owned(),
            ]
        );
    }

    #[test]
    fn several_discovered_hosts_with_a_terminal_are_prompted() {
        let printers = vec![
            network_discovery([10, 42, 0, 5], 9100),
            network_discovery([10, 42, 0, 71], 9100),
        ];

        let chosen =
            choose_discovered_printer(printers, 9100, true, &mut FirstChoiceDiscoverPicker)
                .expect("the prompted selection should resolve");

        assert_eq!(chosen.host, "10.42.0.5");
        assert_eq!(chosen.port, 9100);
    }
}
