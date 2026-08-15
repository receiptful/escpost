//! Terminal formatting for the structured `printers list` response.

use std::io::Write;

use super::super::Availability;
use super::{ConnectionFacts, Response};
use crate::error::CliError;

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
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::features::printers::list::{NetworkConnectionFacts, Printer, UsbConnectionFacts};
    use crate::features::printers::{Availability, Transport};

    #[test]
    fn rich_typed_response_uses_the_production_list_writer() {
        let response = Response {
            config_path: PathBuf::from("/tmp/printers.toml"),
            printers: vec![
                Printer {
                    name: "netum-usb".to_owned(),
                    transport: Transport::Usb,
                    availability: Availability::Connected,
                    profile: None,
                    connection: ConnectionFacts::Usb(UsbConnectionFacts {
                        vendor_id: 0x0416,
                        product_id: 0x5011,
                        bus: Some("3".to_owned()),
                        address: Some(57),
                        manufacturer: Some("YICHIP3121".to_owned()),
                        product: Some("USB Portable Printer".to_owned()),
                        serial_number: Some("B120300001".to_owned()),
                        interface_number: 0,
                        out_endpoints: vec![0x01, 0x02],
                        in_endpoints: vec![0x81],
                    }),
                },
                Printer {
                    name: "offline-usb".to_owned(),
                    transport: Transport::Usb,
                    availability: Availability::Unavailable,
                    profile: Some("NT-5890K".to_owned()),
                    connection: ConnectionFacts::Usb(UsbConnectionFacts {
                        vendor_id: 0x1234,
                        product_id: 0xabcd,
                        bus: None,
                        address: None,
                        manufacturer: None,
                        product: None,
                        serial_number: Some("OFFLINE".to_owned()),
                        interface_number: 2,
                        out_endpoints: vec![0x03],
                        in_endpoints: Vec::new(),
                    }),
                },
                Printer {
                    name: "kitchen".to_owned(),
                    transport: Transport::Network,
                    availability: Availability::Connected,
                    profile: Some("TM-T88V".to_owned()),
                    connection: ConnectionFacts::Network(NetworkConnectionFacts {
                        host: "2001:db8::5".to_owned(),
                        port: 9100,
                    }),
                },
            ],
        };
        let mut output = Vec::new();

        write_response(&response, &mut output).expect("the typed response should be writable");

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
    endpoints: out 0x01, 0x02; in 0x81
    serial: B120300001
[2] offline-usb
    status: unavailable
    profile: NT-5890K
    transport: usb
    usb: 1234:abcd; interface 2
    endpoints: out 0x03
    serial: OFFLINE
[3] kitchen
    status: connected
    profile: TM-T88V
    transport: network
    network: [2001:db8::5]:9100
"
        );
    }

    #[test]
    fn typed_usb_response_omits_absent_optional_lines() {
        let response = Response {
            config_path: PathBuf::from("/tmp/printers.toml"),
            printers: vec![Printer {
                name: "minimal".to_owned(),
                transport: Transport::Usb,
                availability: Availability::Unavailable,
                profile: None,
                connection: ConnectionFacts::Usb(UsbConnectionFacts {
                    vendor_id: 0x0001,
                    product_id: 0x0002,
                    bus: None,
                    address: None,
                    manufacturer: None,
                    product: None,
                    serial_number: None,
                    interface_number: 0,
                    out_endpoints: vec![0x01],
                    in_endpoints: Vec::new(),
                }),
            }],
        };
        let mut output = Vec::new();

        write_response(&response, &mut output).expect("the typed response should be writable");

        assert_eq!(
            String::from_utf8(output).expect("the listing should be UTF-8"),
            "\
[1] minimal
    status: unavailable
    profile: unassigned
    transport: usb
    usb: 0001:0002; interface 0
    endpoints: out 0x01
"
        );
    }

    #[test]
    fn empty_typed_response_is_a_successful_snapshot() {
        let response = Response {
            config_path: PathBuf::from("/tmp/printers.toml"),
            printers: Vec::new(),
        };
        let mut output = Vec::new();

        write_response(&response, &mut output).expect("the empty response should be writable");

        assert_eq!(output, b"No printers configured.\n");
    }
}
