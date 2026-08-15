//! Structured `printers list` operation.

use std::path::PathBuf;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::application;
use crate::configuration::{self, PrinterConfiguration};

use super::inventory::{MergedUsbIdentities, NusbInventory, UsbInventory, merge_usb_identities};
use super::{Availability, Transport};

pub(crate) mod cli;

const NETWORK_PROBE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) config: Option<PathBuf>,
    pub(crate) transport: Option<Transport>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Response {
    pub(crate) config_path: PathBuf,
    pub(crate) printers: Vec<Printer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Printer {
    pub(crate) name: String,
    pub(crate) transport: Transport,
    pub(crate) availability: Availability,
    pub(crate) profile: Option<String>,
    pub(crate) connection: ConnectionFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConnectionFacts {
    Usb(UsbConnectionFacts),
    Network(NetworkConnectionFacts),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UsbConnectionFacts {
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) bus: Option<String>,
    pub(crate) address: Option<u8>,
    pub(crate) manufacturer: Option<String>,
    pub(crate) product: Option<String>,
    pub(crate) serial_number: Option<String>,
    pub(crate) interface_number: u8,
    pub(crate) out_endpoints: Vec<u8>,
    pub(crate) in_endpoints: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NetworkConnectionFacts {
    pub(crate) host: String,
    pub(crate) port: u16,
}

pub(crate) async fn execute_with_observer(
    request: Request,
    mut on_loaded: impl FnMut(&std::path::Path),
) -> application::Result<Response> {
    let configuration = configuration::load(request.config.as_deref())?;
    let config_path = configuration::resolved_path(request.config.as_deref())?;
    on_loaded(&config_path);
    let network_statuses = if request.transport == Some(Transport::Usb) {
        Vec::new()
    } else {
        probe_network_printers(&configuration).await
    };
    response_from_configuration(
        request,
        &mut NusbInventory,
        &configuration,
        config_path,
        &network_statuses,
    )
}

#[cfg(test)]
fn build_response(
    request: Request,
    inventory: &mut impl UsbInventory,
    network_statuses: &[bool],
) -> application::Result<Response> {
    let configuration = configuration::load(request.config.as_deref())?;
    let config_path = configuration::resolved_path(request.config.as_deref())?;
    response_from_configuration(
        request,
        inventory,
        &configuration,
        config_path,
        network_statuses,
    )
}

fn response_from_configuration(
    request: Request,
    inventory: &mut impl UsbInventory,
    configuration: &PrinterConfiguration,
    config_path: PathBuf,
    network_statuses: &[bool],
) -> application::Result<Response> {
    let identities = if request.transport == Some(Transport::Network)
        || configuration.usb_printers().is_empty()
    {
        Vec::new()
    } else {
        inventory.identities()?
    };
    let usb = merge_usb_identities(identities, configuration);
    let mut printers = structured_printers(
        &usb,
        configuration,
        network_statuses,
        request.transport != Some(Transport::Network),
        request.transport != Some(Transport::Usb),
    );
    printers.sort_by(|left, right| {
        availability_rank(left.availability)
            .cmp(&availability_rank(right.availability))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| transport_rank(left.transport).cmp(&transport_rank(right.transport)))
    });
    Ok(Response {
        config_path,
        printers,
    })
}

fn structured_printers(
    usb: &MergedUsbIdentities,
    configuration: &PrinterConfiguration,
    network_statuses: &[bool],
    include_usb: bool,
    include_network: bool,
) -> Vec<Printer> {
    let mut printers = Vec::new();
    if include_usb {
        for connected in &usb.connected {
            let configured = &configuration.usb_printers()[connected.configuration_index];
            let live = &connected.printer;
            printers.push(Printer {
                name: configured.name.clone(),
                transport: Transport::Usb,
                availability: Availability::Connected,
                profile: configured.profile.clone(),
                connection: ConnectionFacts::Usb(UsbConnectionFacts {
                    vendor_id: live.vendor_id,
                    product_id: live.product_id,
                    bus: Some(live.bus.clone()),
                    address: Some(live.address),
                    manufacturer: live.manufacturer.clone(),
                    product: live.product.clone(),
                    serial_number: live.serial_number.clone(),
                    interface_number: live.interface_number,
                    out_endpoints: live.out_endpoints.clone(),
                    in_endpoints: live.in_endpoints.clone(),
                }),
            });
        }
        for index in &usb.unavailable_configuration_indexes {
            let configured = &configuration.usb_printers()[*index];
            printers.push(Printer {
                name: configured.name.clone(),
                transport: Transport::Usb,
                availability: Availability::Unavailable,
                profile: configured.profile.clone(),
                connection: ConnectionFacts::Usb(UsbConnectionFacts {
                    vendor_id: configured.vendor_id,
                    product_id: configured.product_id,
                    bus: None,
                    address: None,
                    manufacturer: None,
                    product: None,
                    serial_number: configured.serial_number.clone(),
                    interface_number: configured.interface_number,
                    out_endpoints: vec![configured.out_endpoint],
                    in_endpoints: configured.in_endpoint.into_iter().collect(),
                }),
            });
        }
    }
    if include_network {
        printers.extend(configuration.network_printers().iter().enumerate().map(
            |(index, configured)| Printer {
                name: configured.name.clone(),
                transport: Transport::Network,
                availability: if network_statuses.get(index).copied().unwrap_or(false) {
                    Availability::Connected
                } else {
                    Availability::Unavailable
                },
                profile: configured.profile.clone(),
                connection: ConnectionFacts::Network(NetworkConnectionFacts {
                    host: configured.host.clone(),
                    port: configured.port,
                }),
            },
        ));
    }
    printers
}

async fn probe_network_printers(configuration: &PrinterConfiguration) -> Vec<bool> {
    let mut probes = JoinSet::new();
    for (index, printer) in configuration.network_printers().iter().enumerate() {
        let host = printer.host.clone();
        let port = printer.port;
        probes.spawn(async move {
            let connected = timeout(
                NETWORK_PROBE_TIMEOUT,
                TcpStream::connect((host.as_str(), port)),
            )
            .await
            .is_ok_and(|result| result.is_ok());
            (index, connected)
        });
    }
    let mut statuses = vec![false; configuration.network_printers().len()];
    while let Some(result) = probes.join_next().await {
        if let Ok((index, connected)) = result {
            statuses[index] = connected;
        }
    }
    statuses
}

fn availability_rank(availability: Availability) -> u8 {
    match availability {
        Availability::Connected => 0,
        Availability::Unavailable => 1,
    }
}

fn transport_rank(transport: Transport) -> u8 {
    match transport {
        Transport::Usb => 0,
        Transport::Network => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::printers::test_support::{
        FixedInventory, netum_usb_printer, temporary_configuration,
    };
    use crate::features::printers::{Availability, Transport};

    #[test]
    fn list_returns_structured_configured_printers_in_status_name_transport_order() {
        let configuration = temporary_configuration(
            "typed-list",
            r#"
[Zulu]
transport = "network"
host = "10.42.0.71"
port = 9100

[alpha]
transport = "usb"
vendor_id = "0x0416"
product_id = "0x5011"
interface_number = 0
out_endpoint = "0x01"

[Bravo]
transport = "network"
host = "10.42.0.72"
port = 9100
"#,
        );
        let mut connected = netum_usb_printer(vec![0x04], vec![0x84]);
        connected.interface_number = 7;
        connected.serial_number = None;
        let mut unconfigured = connected.clone();
        unconfigured.vendor_id = 0x9999;
        unconfigured.product_id = 0x0001;
        unconfigured.address = 12;
        let mut inventory = FixedInventory {
            printers: vec![unconfigured, connected],
        };

        let response = build_response(
            Request {
                config: Some(configuration.path().to_owned()),
                transport: None,
            },
            &mut inventory,
            &[true, false],
        )
        .expect("the structured list should be built");

        assert_eq!(
            response
                .printers
                .iter()
                .map(|printer| (
                    printer.name.as_str(),
                    printer.availability,
                    printer.transport
                ))
                .collect::<Vec<_>>(),
            vec![
                ("alpha", Availability::Connected, Transport::Usb),
                ("Bravo", Availability::Connected, Transport::Network),
                ("Zulu", Availability::Unavailable, Transport::Network),
            ]
        );

        let ConnectionFacts::Usb(usb) = &response.printers[0].connection else {
            panic!("alpha should have USB connection facts");
        };
        assert_eq!(usb.bus.as_deref(), Some("003"));
        assert_eq!(usb.address, Some(60));
        assert_eq!(usb.manufacturer.as_deref(), Some("YICHIP3121"));
        assert_eq!(usb.product.as_deref(), Some("USB Portable Printer"));
        assert_eq!(usb.interface_number, 0);
        assert_eq!(usb.out_endpoints, vec![0x01]);
        assert!(usb.in_endpoints.is_empty());
    }

    #[test]
    fn list_requires_an_exact_configured_serial_match() {
        let configuration = temporary_configuration(
            "list-serial-mismatch",
            r#"
[counter]
transport = "usb"
vendor_id = "0x0416"
product_id = "0x5011"
serial_number = "EXPECTED"
interface_number = 2
out_endpoint = "0x03"
in_endpoint = "0x83"
"#,
        );
        let mut connected = netum_usb_printer(vec![0x03], vec![0x83]);
        connected.serial_number = Some("DIFFERENT".to_owned());
        let mut inventory = FixedInventory {
            printers: vec![connected],
        };

        let response = build_response(
            Request {
                config: Some(configuration.path().to_owned()),
                transport: None,
            },
            &mut inventory,
            &[],
        )
        .expect("the configured printer should be reported as unavailable");

        assert_eq!(response.printers.len(), 1);
        assert_eq!(response.printers[0].name, "counter");
        assert_eq!(response.printers[0].availability, Availability::Unavailable);
        let ConnectionFacts::Usb(usb) = &response.printers[0].connection else {
            panic!("counter should have USB connection facts");
        };
        assert_eq!(usb.bus, None);
        assert_eq!(usb.address, None);
        assert_eq!(usb.serial_number.as_deref(), Some("EXPECTED"));
        assert_eq!(usb.interface_number, 2);
        assert_eq!(usb.out_endpoints, vec![0x03]);
        assert_eq!(usb.in_endpoints, vec![0x83]);
    }

    #[test]
    fn list_uses_one_deterministic_alias_for_an_ambiguous_usb_identity() {
        let configuration = temporary_configuration(
            "list-ambiguous-aliases",
            r#"
[Zulu]
transport = "usb"
vendor_id = "0x0416"
product_id = "0x5011"
interface_number = 0
out_endpoint = "0x01"

[alpha]
transport = "usb"
vendor_id = "0x0416"
product_id = "0x5011"
interface_number = 0
out_endpoint = "0x01"
"#,
        );
        let mut connected = netum_usb_printer(vec![0x01], vec![0x81]);
        connected.serial_number = None;
        let mut inventory = FixedInventory {
            printers: vec![connected],
        };

        let response = build_response(
            Request {
                config: Some(configuration.path().to_owned()),
                transport: None,
            },
            &mut inventory,
            &[],
        )
        .expect("ambiguous aliases should be merged deterministically");

        assert_eq!(response.printers.len(), 1);
        assert_eq!(response.printers[0].name, "alpha");
        assert_eq!(response.printers[0].availability, Availability::Connected);
    }

    #[test]
    fn list_transport_filters_return_only_the_requested_transport() {
        let configuration = temporary_configuration(
            "list-transport-filter",
            r#"
[counter]
transport = "usb"
vendor_id = "0x0416"
product_id = "0x5011"
interface_number = 0
out_endpoint = "0x01"

[kitchen]
transport = "network"
host = "10.42.0.71"
port = 9100
"#,
        );
        let mut connected = netum_usb_printer(vec![0x01], vec![0x81]);
        connected.serial_number = None;
        let mut inventory = FixedInventory {
            printers: vec![connected],
        };

        let network = build_response(
            Request {
                config: Some(configuration.path().to_owned()),
                transport: Some(Transport::Network),
            },
            &mut inventory,
            &[true],
        )
        .expect("network-only list should be built");
        assert_eq!(network.printers.len(), 1);
        assert_eq!(network.printers[0].name, "kitchen");
        assert_eq!(network.printers[0].transport, Transport::Network);

        let usb = build_response(
            Request {
                config: Some(configuration.path().to_owned()),
                transport: Some(Transport::Usb),
            },
            &mut inventory,
            &[],
        )
        .expect("USB-only list should be built");
        assert_eq!(usb.printers.len(), 1);
        assert_eq!(usb.printers[0].name, "counter");
        assert_eq!(usb.printers[0].transport, Transport::Usb);
    }
}
