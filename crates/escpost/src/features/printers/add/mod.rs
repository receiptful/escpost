//! Persist a fully resolved printer registration request.

use std::path::PathBuf;

use crate::application::{self, ApplicationError};
use crate::configuration::{self, UsbPrinterRegistration};

use super::Connection;

pub(crate) mod cli;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub(crate) config: Option<PathBuf>,
    pub(crate) name: String,
    pub(crate) profile: Option<String>,
    pub(crate) connection: Connection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Response {
    pub(crate) config_path: PathBuf,
    pub(crate) printer_name: String,
    pub(crate) connection: Connection,
}

pub(crate) fn execute(request: Request) -> application::Result<Response> {
    validate(&request)?;
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
        connection: request.connection,
    })
}

fn validate(request: &Request) -> application::Result<()> {
    if request.name.trim().is_empty() {
        return Err(ApplicationError::BlankPrinterName);
    }
    if request
        .profile
        .as_deref()
        .is_some_and(|profile| profile.trim().is_empty())
    {
        return Err(ApplicationError::BlankPrinterProfile);
    }
    if let Connection::Network { host, port } = &request.connection {
        if host.trim().is_empty() {
            return Err(ApplicationError::BlankPrinterHost);
        }
        if *port == 0 {
            return Err(ApplicationError::InvalidPrinterPort);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::printers::Connection;
    use crate::features::printers::test_support::temporary_configuration;

    #[test]
    fn add_persists_a_fully_resolved_network_request_and_returns_saved_facts() {
        let configuration = temporary_configuration("typed-add", "");
        let connection = Connection::Network {
            host: "10.42.0.71".to_owned(),
            port: 9200,
        };

        let response = execute(Request {
            config: Some(configuration.path().to_owned()),
            name: "kitchen".to_owned(),
            profile: Some("REFERENCE".to_owned()),
            connection: connection.clone(),
        })
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
}
