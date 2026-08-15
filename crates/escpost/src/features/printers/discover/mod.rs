//! Structured USB and network discovery operation.

use std::num::NonZeroU16;
use std::path::PathBuf;
use std::time::Duration;

use crate::application::{self, ApplicationError};
use crate::configuration::{self, PrinterConfiguration};
use crate::discovery::{self, DiscoveredHost, ScanTarget, Subnet};

use super::inventory::{
    NusbInventory, UsbEnumerationFailure, UsbInventory, UsbPrinter, classify_usb_printers,
    sort_by_usb_location,
};

pub(crate) mod cli;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DiscoveryScope {
    Usb,
    Network(NetworkScan),
    All(NetworkScan),
}

impl DiscoveryScope {
    pub(crate) fn network_scan(&self) -> Option<&NetworkScan> {
        match self {
            Self::Usb => None,
            Self::Network(scan) | Self::All(scan) => Some(scan),
        }
    }

    fn includes_usb(&self) -> bool {
        matches!(self, Self::Usb | Self::All(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NetworkScan {
    port: NonZeroU16,
    subnets: Vec<Subnet>,
    timeout: Duration,
}

impl NetworkScan {
    pub(crate) fn new(
        port: u16,
        subnets: Vec<Subnet>,
        timeout: Duration,
    ) -> application::Result<Self> {
        let port = NonZeroU16::new(port).ok_or(ApplicationError::InvalidPrinterPort)?;
        Ok(Self {
            port,
            subnets,
            timeout,
        })
    }

    pub(crate) fn port(&self) -> u16 {
        self.port.get()
    }

    pub(crate) fn subnets(&self) -> &[Subnet] {
        &self.subnets
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    pub(crate) fn uses_automatic_subnets(&self) -> bool {
        self.subnets.is_empty()
    }
}

pub(crate) struct PreparedDiscovery {
    configuration: PrinterConfiguration,
    config_path: PathBuf,
    scope: DiscoveryScope,
    scan_targets: Vec<ScanTarget>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum DiscoveryEvent<'a> {
    Prepared {
        config_path: &'a std::path::Path,
        scope: &'a DiscoveryScope,
        scan_targets: &'a [ScanTarget],
    },
    NetworkScanProgress {
        completed: u64,
        total: u64,
    },
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

pub(crate) fn prepare(
    config: Option<PathBuf>,
    scope: DiscoveryScope,
) -> application::Result<PreparedDiscovery> {
    let configuration = configuration::load_for_update(config.as_deref())?;
    let config_path = configuration::resolved_path(config.as_deref())?;
    let scan_targets = match scope.network_scan() {
        Some(scan) => discovery_targets(scan.subnets())?,
        None => Vec::new(),
    };
    Ok(PreparedDiscovery {
        configuration,
        config_path,
        scope,
        scan_targets,
    })
}

pub(crate) async fn execute(
    prepared: PreparedDiscovery,
    mut observer: impl FnMut(DiscoveryEvent<'_>),
) -> application::Result<Response> {
    observer(DiscoveryEvent::Prepared {
        config_path: &prepared.config_path,
        scope: &prepared.scope,
        scan_targets: &prepared.scan_targets,
    });
    let hosts = if let Some(scan) = prepared.scope.network_scan() {
        discovery::scan(
            &prepared.scan_targets,
            scan.port(),
            scan.timeout(),
            |completed, total| {
                observer(DiscoveryEvent::NetworkScanProgress { completed, total });
            },
        )
        .await
    } else {
        Vec::new()
    };
    response_from_prepared(prepared, &mut NusbInventory, hosts)
}

#[cfg(test)]
fn build_response(
    config: Option<PathBuf>,
    scope: DiscoveryScope,
    inventory: &mut impl UsbInventory,
    hosts: Vec<DiscoveredHost>,
) -> application::Result<Response> {
    response_from_prepared(prepare(config, scope)?, inventory, hosts)
}

fn response_from_prepared(
    prepared: PreparedDiscovery,
    inventory: &mut impl UsbInventory,
    hosts: Vec<DiscoveredHost>,
) -> application::Result<Response> {
    let enumeration = if prepared.scope.includes_usb() {
        inventory.list_tolerant()?
    } else {
        super::inventory::UsbEnumeration {
            printers: Vec::new(),
            failures: Vec::new(),
        }
    };
    let mut printers = enumeration.printers;
    sort_by_usb_location(&mut printers);
    let connected = classify_usb_printers(printers, &prepared.configuration).0;
    let usb_printers = connected
        .into_iter()
        .map(|connected| {
            let configured = connected
                .configuration_index
                .map(|index| &prepared.configuration.usb_printers()[index]);
            UsbDiscovery {
                configured_name: configured.map(|printer| printer.name.clone()),
                configured_profile: configured.and_then(|printer| printer.profile.clone()),
                printer: connected.printer,
            }
        })
        .collect::<Vec<_>>();
    let network_printers = if prepared.scope.network_scan().is_none() {
        Vec::new()
    } else {
        hosts
            .into_iter()
            .map(|host| {
                let configured_names = configured_names(&prepared.configuration, &host);
                let configured_profile = prepared
                    .configuration
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
        config_path: prepared.config_path,
        scan_targets: prepared.scan_targets,
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
            return Err(ApplicationError::NoDiscoverableSubnets);
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
    use crate::features::printers::test_support::{
        TolerantInventory, discovered, netum_usb_printer, temporary_configuration,
    };
    use std::net::Ipv4Addr;
    use std::time::Duration;

    fn explicit_network_scan() -> NetworkScan {
        NetworkScan::new(
            9100,
            vec![Subnet::parse("10.42.0.0/24").expect("valid subnet")],
            Duration::from_millis(50),
        )
        .expect("the explicit network scan should be valid")
    }

    #[test]
    fn usb_scope_enumerates_usb_without_accepting_network_results() {
        let configuration = temporary_configuration("discover-usb-scope", "");
        let prepared = prepare(Some(configuration.path().to_owned()), DiscoveryScope::Usb)
            .expect("USB discovery should prepare");
        let mut inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
                failures: Vec::new(),
            }),
        };

        let response = response_from_prepared(
            prepared,
            &mut inventory,
            vec![discovered([10, 42, 0, 71], 9100)],
        )
        .expect("USB discovery should build its response");

        assert_eq!(response.usb_printers.len(), 1);
        assert!(response.network_printers.is_empty());
        assert!(response.scan_targets.is_empty());
    }

    #[test]
    fn network_scope_scans_network_without_enumerating_usb() {
        let configuration = temporary_configuration("discover-network-scope", "");
        let prepared = prepare(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Network(explicit_network_scan()),
        )
        .expect("network discovery should prepare");
        let mut inventory = TolerantInventory { enumeration: None };

        let response = response_from_prepared(
            prepared,
            &mut inventory,
            vec![discovered([10, 42, 0, 71], 9100)],
        )
        .expect("network discovery should build its response");

        assert!(response.usb_printers.is_empty());
        assert_eq!(response.network_printers.len(), 1);
        assert_eq!(response.scan_targets.len(), 1);
    }

    #[test]
    fn all_scope_combines_usb_and_network_results() {
        let configuration = temporary_configuration("discover-all-scope", "");
        let prepared = prepare(
            Some(configuration.path().to_owned()),
            DiscoveryScope::All(explicit_network_scan()),
        )
        .expect("combined discovery should prepare");
        let mut inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
                failures: Vec::new(),
            }),
        };

        let response = response_from_prepared(
            prepared,
            &mut inventory,
            vec![discovered([10, 42, 0, 71], 9100)],
        )
        .expect("combined discovery should build its response");

        assert_eq!(response.usb_printers.len(), 1);
        assert_eq!(response.network_printers.len(), 1);
        assert_eq!(response.scan_targets.len(), 1);
    }

    #[test]
    fn discover_returns_structured_results_and_tolerant_usb_failure_facts_without_output() {
        let configuration = temporary_configuration("typed-discover", "");
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

        let response = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::All(explicit_network_scan()),
            &mut inventory,
            hosts,
        )
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

    #[test]
    fn discover_transforms_configured_and_new_usb_and_network_results() {
        let configuration = temporary_configuration(
            "discover-transformation",
            r#"
[counter]
transport = "usb"
vendor_id = "0x0416"
product_id = "0x5011"
serial_number = "B120300001"
interface_number = 0
out_endpoint = "0x01"
profile = "NT-5890K"

[kitchen]
transport = "network"
host = "10.42.0.71"
port = 9100
profile = "TM-T88V"

[kitchen-alias]
transport = "network"
host = "10.42.0.71"
port = 9100
"#,
        );
        let configured_usb = netum_usb_printer(vec![0x01], vec![0x81]);
        let mut new_usb = netum_usb_printer(vec![0x02], vec![0x82]);
        new_usb.vendor_id = 0x1234;
        new_usb.product_id = 0xabcd;
        new_usb.bus = "001".to_owned();
        new_usb.address = 7;
        new_usb.serial_number = None;
        let mut inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: vec![configured_usb, new_usb],
                failures: Vec::new(),
            }),
        };
        let hosts = vec![
            discovered([10, 42, 0, 71], 9100),
            discovered([10, 42, 0, 72], 9100),
        ];

        let response = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::All(explicit_network_scan()),
            &mut inventory,
            hosts,
        )
        .expect("the typed discovery response should be built");

        assert_eq!(response.usb_printers.len(), 2);
        let configured_usb = response
            .usb_printers
            .iter()
            .find(|printer| printer.configured_name.as_deref() == Some("counter"))
            .expect("the configured USB printer should be classified");
        assert_eq!(
            configured_usb.configured_profile.as_deref(),
            Some("NT-5890K")
        );
        assert!(
            response
                .usb_printers
                .iter()
                .any(|printer| printer.configured_name.is_none())
        );
        assert_eq!(response.network_printers.len(), 2);
        assert_eq!(
            response.network_printers[0].configured_names,
            vec!["kitchen".to_owned(), "kitchen-alias".to_owned()]
        );
        assert_eq!(
            response.network_printers[0].configured_profile.as_deref(),
            Some("TM-T88V")
        );
        assert!(response.network_printers[1].configured_names.is_empty());
        assert!(response.registration.usb);
        assert!(response.registration.network);
    }

    #[test]
    fn discover_transport_filters_skip_the_other_transport() {
        let configuration = temporary_configuration("discover-transport-filter", "");
        let hosts = vec![discovered([10, 42, 0, 71], 9100)];
        let mut network_inventory = TolerantInventory { enumeration: None };

        let network = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Network(explicit_network_scan()),
            &mut network_inventory,
            hosts.clone(),
        )
        .expect("network-only discovery should not enumerate USB");
        assert!(network.usb_printers.is_empty());
        assert_eq!(network.network_printers.len(), 1);

        let mut usb_inventory = TolerantInventory {
            enumeration: Some(UsbEnumeration {
                printers: vec![netum_usb_printer(vec![0x01], vec![0x81])],
                failures: Vec::new(),
            }),
        };
        let usb = build_response(
            Some(configuration.path().to_owned()),
            DiscoveryScope::Usb,
            &mut usb_inventory,
            hosts,
        )
        .expect("USB-only discovery should ignore network hosts");
        assert_eq!(usb.usb_printers.len(), 1);
        assert!(usb.network_printers.is_empty());
    }
}
