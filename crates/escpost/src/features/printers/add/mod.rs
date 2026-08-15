//! Persist a fully resolved printer registration request.

use std::path::PathBuf;

use crate::application::{self, ApplicationError};
use crate::configuration::{self, UsbPrinterRegistration};

pub(crate) mod cli;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::printers::test_support::temporary_configuration;

    #[test]
    fn request_constructor_rejects_a_blank_printer_name() {
        let error = Request::new(None, " \t".to_owned(), None, network_connection())
            .expect_err("a blank printer name must not form an add request");

        assert!(matches!(error, ApplicationError::BlankPrinterName));
    }

    #[test]
    fn request_constructor_rejects_a_blank_printer_profile() {
        let error = Request::new(
            None,
            "kitchen".to_owned(),
            Some("\n ".to_owned()),
            network_connection(),
        )
        .expect_err("a blank printer profile must not form an add request");

        assert!(matches!(error, ApplicationError::BlankPrinterProfile));
    }

    #[test]
    fn network_connection_constructor_rejects_a_blank_host() {
        let error = Connection::network("  ".to_owned(), 9100)
            .expect_err("a blank host must not form a network connection");

        assert!(matches!(error, ApplicationError::BlankPrinterHost));
    }

    #[test]
    fn network_connection_constructor_rejects_port_zero() {
        let error = Connection::network("10.42.0.71".to_owned(), 0)
            .expect_err("port zero must not form a network connection");

        assert!(matches!(error, ApplicationError::InvalidPrinterPort));
    }

    #[test]
    fn usb_connection_constructor_rejects_a_blank_serial_number() {
        let error = Connection::usb(0x0416, 0x5011, Some(" \t".to_owned()), 0, 0x01, Some(0x81))
            .expect_err("a blank serial number must not form a USB connection");

        assert!(matches!(error, ApplicationError::BlankUsbSerialNumber));
    }

    #[test]
    fn usb_connection_constructor_rejects_an_invalid_out_endpoint() {
        let error = Connection::usb(0x0416, 0x5011, None, 0, 0x81, Some(0x81))
            .expect_err("an IN endpoint must not form a USB OUT connection");

        assert!(matches!(
            error,
            ApplicationError::InvalidUsbOutEndpoint(0x81)
        ));
    }

    #[test]
    fn usb_connection_constructor_rejects_an_invalid_in_endpoint() {
        let error = Connection::usb(0x0416, 0x5011, None, 0, 0x01, Some(0x01))
            .expect_err("an OUT endpoint must not form a USB IN connection");

        assert!(matches!(
            error,
            ApplicationError::InvalidUsbInEndpoint(0x01)
        ));
    }

    #[test]
    fn constructors_form_valid_network_and_usb_add_requests() {
        let network = Connection::network("10.42.0.71".to_owned(), 9200)
            .expect("a valid host and port should form a network connection");
        Request::new(
            None,
            "kitchen".to_owned(),
            Some("REFERENCE".to_owned()),
            network,
        )
        .expect("valid network registration facts should form an add request");
        let usb = Connection::usb(
            0x0416,
            0x5011,
            Some("B120300001".to_owned()),
            0,
            0x01,
            Some(0x81),
        )
        .expect("valid USB descriptor coordinates should form a USB connection");
        Request::new(None, "counter".to_owned(), None, usb)
            .expect("valid USB registration facts should form an add request");
    }

    #[test]
    fn add_persists_a_fully_resolved_network_request_and_returns_saved_facts() {
        let configuration = temporary_configuration("typed-add", "");
        let connection = network_connection();

        let response = execute(
            Request::new(
                Some(configuration.path().to_owned()),
                "kitchen".to_owned(),
                Some("REFERENCE".to_owned()),
                connection.clone(),
            )
            .expect("valid registration facts should form an add request"),
        )
        .expect("the resolved request should be saved");

        assert_eq!(response.config_path, configuration.path());
        assert_eq!(response.printer_name, "kitchen");
        assert_eq!(response.connection, connection);
        let saved = std::fs::read_to_string(configuration.path())
            .expect("the saved configuration should be readable");
        assert!(saved.contains("[kitchen]"));
        assert!(saved.contains("host = \"10.42.0.71\""));
        assert!(saved.contains("port = 9200"));
    }

    fn network_connection() -> Connection {
        Connection::network("10.42.0.71".to_owned(), 9200)
            .expect("valid network facts should form a connection")
    }
}
