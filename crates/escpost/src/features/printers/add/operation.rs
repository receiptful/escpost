use std::path::PathBuf;

use crate::application::{self, ApplicationError};
use crate::configuration::{self, UsbPrinterRegistration};

/// Validated transport coordinates for a printer being registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Connection {
    target: ConnectionTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConnectionTarget {
    Usb {
        vendor_id: u16,
        product_id: u16,
        serial_number: Option<String>,
        interface_number: u8,
        out_endpoint: u8,
        in_endpoint: Option<u8>,
    },
    Network {
        host: String,
        port: u16,
    },
}

impl Connection {
    pub(crate) fn usb(
        vendor_id: u16,
        product_id: u16,
        serial_number: Option<String>,
        interface_number: u8,
        out_endpoint: u8,
        in_endpoint: Option<u8>,
    ) -> application::Result<Self> {
        if serial_number
            .as_deref()
            .is_some_and(|serial_number| serial_number.trim().is_empty())
        {
            return Err(ApplicationError::BlankUsbSerialNumber);
        }
        if !(0x01..=0x0f).contains(&out_endpoint) {
            return Err(ApplicationError::InvalidUsbOutEndpoint(out_endpoint));
        }
        if let Some(in_endpoint) = in_endpoint
            && !(0x81..=0x8f).contains(&in_endpoint)
        {
            return Err(ApplicationError::InvalidUsbInEndpoint(in_endpoint));
        }

        Ok(Self {
            target: ConnectionTarget::Usb {
                vendor_id,
                product_id,
                serial_number,
                interface_number,
                out_endpoint,
                in_endpoint,
            },
        })
    }

    pub(crate) fn network(host: String, port: u16) -> application::Result<Self> {
        if host.trim().is_empty() {
            return Err(ApplicationError::BlankPrinterHost);
        }
        if port == 0 {
            return Err(ApplicationError::InvalidPrinterPort);
        }

        Ok(Self {
            target: ConnectionTarget::Network { host, port },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    config: Option<PathBuf>,
    name: String,
    profile: Option<String>,
    connection: Connection,
}

impl Request {
    pub(crate) fn new(
        config: Option<PathBuf>,
        name: String,
        profile: Option<String>,
        connection: Connection,
    ) -> application::Result<Self> {
        if name.trim().is_empty() {
            return Err(ApplicationError::BlankPrinterName);
        }
        if profile
            .as_deref()
            .is_some_and(|profile| profile.trim().is_empty())
        {
            return Err(ApplicationError::BlankPrinterProfile);
        }

        Ok(Self {
            config,
            name,
            profile,
            connection,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Response {
    pub(crate) config_path: PathBuf,
    pub(crate) printer_name: String,
    pub(crate) connection: Connection,
}

pub(crate) fn execute(request: Request) -> application::Result<Response> {
    let config_path = match &request.connection.target {
        ConnectionTarget::Network { host, port } => configuration::add_network_printer(
            request.config.as_deref(),
            &request.name,
            host,
            *port,
            request.profile.as_deref(),
        ),
        ConnectionTarget::Usb {
            vendor_id,
            product_id,
            serial_number,
            interface_number,
            out_endpoint,
            in_endpoint,
        } => configuration::add_usb_printer(
            request.config.as_deref(),
            &request.name,
            &UsbPrinterRegistration {
                vendor_id: *vendor_id,
                product_id: *product_id,
                serial_number: serial_number.as_deref(),
                interface_number: *interface_number,
                out_endpoint: *out_endpoint,
                in_endpoint: *in_endpoint,
                profile: request.profile.as_deref(),
            },
        ),
    }?;

    Ok(Response {
        config_path,
        printer_name: request.name,
        connection: request.connection,
    })
}
