use std::path::PathBuf;

use crate::application::{self, ApplicationError};
use crate::configuration::{self, UsbPrinterRegistration};

/// Advisory shown when a USB printer is registered without a serial number:
/// the vendor/product descriptor alone cannot tell it apart from another
/// unit of the same make and model, so printing may reach the wrong
/// physical device while both are connected. Shared by the CLI and HTTP
/// adapters, which each decide *when* to show it, so the wording itself
/// cannot drift between the two.
pub(crate) const AMBIGUOUS_USB_WARNING: &str = "This printer reports no serial number. Printing will be ambiguous while another device with the same USB identity is connected.";

/// The RAW TCP port a registration falls back to when none is given: the
/// interactive prompt's default and the non-interactive fallback are the same
/// number, and the workbench's manual dialog starts from it too. The
/// workbench cannot import it, so `add::tests` checks its copy against this
/// one — see the test for why that drift is worth catching.
pub(crate) const DEFAULT_RAW_PORT: u16 = 9100;

/// Desired transport coordinates for a printer being registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Connection {
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
    pub(crate) fn transport(&self) -> &'static str {
        match self {
            Self::Usb { .. } => "usb",
            Self::Network { .. } => "network",
        }
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
        match &connection {
            Connection::Network { host, port } => {
                if host.trim().is_empty() {
                    return Err(ApplicationError::BlankPrinterHost);
                }
                if *port == 0 {
                    return Err(ApplicationError::InvalidPrinterPort);
                }
            }
            Connection::Usb {
                serial_number,
                out_endpoint,
                in_endpoint,
                ..
            } => {
                if serial_number
                    .as_deref()
                    .is_some_and(|serial_number| serial_number.trim().is_empty())
                {
                    return Err(ApplicationError::BlankUsbSerialNumber);
                }
                if !(0x01..=0x0f).contains(out_endpoint) {
                    return Err(ApplicationError::InvalidUsbOutEndpoint(*out_endpoint));
                }
                if let Some(in_endpoint) = in_endpoint
                    && !(0x81..=0x8f).contains(in_endpoint)
                {
                    return Err(ApplicationError::InvalidUsbInEndpoint(*in_endpoint));
                }
            }
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
    pub(crate) profile: Option<String>,
    pub(crate) connection: Connection,
}

pub(crate) fn execute(request: Request) -> application::Result<Response> {
    let config_path = match &request.connection {
        Connection::Network { host, port } => configuration::add_network_printer(
            request.config.as_deref(),
            &request.name,
            host,
            *port,
            request.profile.as_deref(),
        ),
        Connection::Usb {
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
        profile: request.profile,
        connection: request.connection,
    })
}
