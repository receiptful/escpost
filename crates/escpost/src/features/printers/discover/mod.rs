//! Structured USB and network discovery operation.

use std::path::PathBuf;
use std::time::Duration;

use crate::application;
use crate::configuration::{self, PrinterConfiguration};
use crate::discovery::{self, DiscoveredHost, ScanTarget, Subnet};
use crate::error::CliError;

use super::Transport;
use super::inventory::{
    NusbInventory, UsbEnumerationFailure, UsbInventory, UsbPrinter, classify_usb_printers,
    sort_by_usb_location,
};

pub(crate) mod cli;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) config: Option<PathBuf>,
    pub(crate) transport: Option<Transport>,
    pub(crate) port: u16,
    pub(crate) subnets: Vec<Subnet>,
    pub(crate) timeout: Duration,
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

pub(crate) async fn execute_with_observer(
    request: Request,
    preflight: impl FnOnce() -> application::Result<()>,
    mut on_ready: impl FnMut(&std::path::Path, &[ScanTarget], bool),
    on_progress: impl FnMut(u64, u64),
) -> application::Result<Response> {
    let configuration = configuration::load_for_update(request.config.as_deref())?;
    let config_path = configuration::resolved_path(request.config.as_deref())?;
    preflight()?;
    if request.port == 0 {
        return Err(CliError::InvalidPrinterPort);
    }
    let targets = if request.transport == Some(Transport::Usb) {
        Vec::new()
    } else {
        discovery_targets(&request.subnets)?
    };
    on_ready(&config_path, &targets, request.subnets.is_empty());
    let hosts = if request.transport == Some(Transport::Usb) {
        Vec::new()
    } else {
        discovery::scan(&targets, request.port, request.timeout, on_progress).await
    };
    response_from_configuration(
        request,
        &mut NusbInventory,
        configuration,
        config_path,
        targets,
        hosts,
    )
}

#[cfg(test)]
fn build_response(
    request: Request,
    inventory: &mut impl UsbInventory,
    scan_targets: Vec<ScanTarget>,
    hosts: Vec<DiscoveredHost>,
) -> application::Result<Response> {
    let configuration = configuration::load_for_update(request.config.as_deref())?;
    let config_path = configuration::resolved_path(request.config.as_deref())?;
    response_from_configuration(
        request,
        inventory,
        configuration,
        config_path,
        scan_targets,
        hosts,
    )
}

fn response_from_configuration(
    request: Request,
    inventory: &mut impl UsbInventory,
    configuration: PrinterConfiguration,
    config_path: PathBuf,
    scan_targets: Vec<ScanTarget>,
    hosts: Vec<DiscoveredHost>,
) -> application::Result<Response> {
    let enumeration = if request.transport == Some(Transport::Network) {
        super::inventory::UsbEnumeration {
            printers: Vec::new(),
            failures: Vec::new(),
        }
    } else {
        inventory.list_tolerant()?
    };
    let mut printers = enumeration.printers;
    sort_by_usb_location(&mut printers);
    let connected = classify_usb_printers(printers, &configuration).0;
    let usb_printers = connected
        .into_iter()
        .map(|connected| {
            let configured = connected
                .configuration_index
                .map(|index| &configuration.usb_printers()[index]);
            UsbDiscovery {
                configured_name: configured.map(|printer| printer.name.clone()),
                configured_profile: configured.and_then(|printer| printer.profile.clone()),
                printer: connected.printer,
            }
        })
        .collect::<Vec<_>>();
    let network_printers = if request.transport == Some(Transport::Usb) {
        Vec::new()
    } else {
        hosts
            .into_iter()
            .map(|host| {
                let configured_names = configured_names(&configuration, &host);
                let configured_profile = configuration
                    .network_printers()
                    .iter()
                    .find(|printer| {
                        printer.port == host.port && printer.host == host.address.to_string()
                    })
                    .and_then(|printer| printer.profile.clone());
                NetworkDiscovery {
                    configured_names,
                    configured_profile,
                    host: host.address.to_string(),
                    port: host.port,
                    interface: host.interface,
                }
            })
            .collect::<Vec<_>>()
    };
    let registration = RegistrationAvailability {
        usb: usb_printers
            .iter()
            .any(|printer| printer.configured_name.is_none()),
        network: network_printers
            .iter()
            .any(|printer| printer.configured_names.is_empty()),
    };
    Ok(Response {
        config_path,
        scan_targets,
        usb_printers,
        network_printers,
        usb_failures: enumeration.failures,
        registration,
    })
}

pub(crate) fn discovery_targets(subnets: &[Subnet]) -> application::Result<Vec<ScanTarget>> {
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
    use crate::discovery::{DiscoveredHost, ScanTarget, Subnet};
    use crate::features::printers::inventory::{
        UsbEnumeration, UsbEnumerationFailure, UsbFailureStage,
    };
    use crate::features::printers::test_support::{TolerantInventory, temporary_configuration};
    use std::net::Ipv4Addr;
    use std::time::Duration;

    #[test]
    fn discover_returns_structured_results_and_tolerant_usb_failure_facts_without_output() {
        let configuration = temporary_configuration("typed-discover", "");
        let request = Request {
            config: Some(configuration.path().to_owned()),
            transport: None,
            port: 9100,
            subnets: vec![Subnet::parse("10.42.0.0/24").expect("valid subnet")],
            timeout: Duration::from_millis(50),
        };
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
        let targets = vec![ScanTarget {
            subnet: Subnet::parse("10.42.0.0/24").expect("valid subnet"),
            interface: None,
            excluded: None,
        }];
        let hosts = vec![DiscoveredHost {
            address: Ipv4Addr::new(10, 42, 0, 71),
            port: 9100,
            interface: Some("enx0".to_owned()),
        }];

        let response = build_response(request, &mut inventory, targets.clone(), hosts)
            .expect("partial USB failure should not abort discovery");

        assert_eq!(response.scan_targets, targets);
        assert_eq!(response.usb_failures.len(), 1);
        assert_eq!(response.usb_failures[0].stage, UsbFailureStage::OpenDevice);
        assert_eq!(response.usb_failures[0].vendor_id, 0x0416);
        assert_eq!(response.usb_failures[0].product_id, 0x5011);
        assert_eq!(response.usb_failures[0].reason, "denied");
        assert!(response.usb_failures[0].permission_denied);
        assert_eq!(response.network_printers.len(), 1);
        assert_eq!(response.network_printers[0].host, "10.42.0.71");
        assert!(response.registration.network);
    }
}
