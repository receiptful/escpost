//! Persist a fully resolved printer registration request.

pub(crate) mod cli;
pub(crate) mod http;
mod operation;

pub(crate) use operation::{
    AMBIGUOUS_USB_WARNING, Connection, DEFAULT_RAW_PORT, Request, Response, execute,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ApplicationError;
    use crate::features::printers::test_support::temporary_configuration;

    /// The workbench's manual registration dialog offers the same port before
    /// anything is typed, as a TypeScript literal it has no way to import.
    /// This is the link between the two copies, and it is the one duplicated
    /// registration constant worth a mechanical guard: a wrong warning or a
    /// wrong hint is visible on screen, while a wrong default port is
    /// accepted by every layer and saved. The printer then lists as
    /// unavailable long afterwards, with nothing pointing back at a number
    /// nobody chose.
    #[test]
    fn the_workbench_dialog_offers_the_same_default_port() {
        let dialog =
            include_str!("../../../../frontend/src/features/printers/add-printer-dialog.tsx");
        let declaration = format!("const DEFAULT_RAW_PORT = {DEFAULT_RAW_PORT};");

        assert!(
            dialog.contains(&declaration),
            "add-printer-dialog.tsx must declare `{declaration}`"
        );
    }

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
    fn request_constructor_rejects_a_blank_network_host() {
        let error = Request::new(
            None,
            "kitchen".to_owned(),
            None,
            Connection::Network {
                host: "  ".to_owned(),
                port: 9100,
            },
        )
        .expect_err("a blank host must not form an add request");

        assert!(matches!(error, ApplicationError::BlankPrinterHost));
    }

    #[test]
    fn request_constructor_rejects_network_port_zero() {
        let error = Request::new(
            None,
            "kitchen".to_owned(),
            None,
            Connection::Network {
                host: "10.42.0.71".to_owned(),
                port: 0,
            },
        )
        .expect_err("port zero must not form an add request");

        assert!(matches!(error, ApplicationError::InvalidPrinterPort));
    }

    #[test]
    fn request_constructor_rejects_a_blank_usb_serial_number() {
        let error = Request::new(
            None,
            "counter".to_owned(),
            None,
            usb_connection(Some(" \t"), 0x01, Some(0x81)),
        )
        .expect_err("a blank serial number must not form an add request");

        assert!(matches!(error, ApplicationError::BlankUsbSerialNumber));
    }

    #[test]
    fn request_constructor_rejects_an_invalid_usb_out_endpoint() {
        let error = Request::new(
            None,
            "counter".to_owned(),
            None,
            usb_connection(None, 0x81, Some(0x81)),
        )
        .expect_err("an IN endpoint must not form an add request");

        assert!(matches!(
            error,
            ApplicationError::InvalidUsbOutEndpoint(0x81)
        ));
    }

    #[test]
    fn request_constructor_rejects_an_invalid_usb_in_endpoint() {
        let error = Request::new(
            None,
            "counter".to_owned(),
            None,
            usb_connection(None, 0x01, Some(0x01)),
        )
        .expect_err("an OUT endpoint must not form an add request");

        assert!(matches!(
            error,
            ApplicationError::InvalidUsbInEndpoint(0x01)
        ));
    }

    #[test]
    fn constructors_form_valid_network_and_usb_add_requests() {
        let network = network_connection();
        Request::new(
            None,
            "kitchen".to_owned(),
            Some("REFERENCE".to_owned()),
            network,
        )
        .expect("valid network registration facts should form an add request");
        let usb = usb_connection(Some("B120300001"), 0x01, Some(0x81));
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
        assert_eq!(response.profile.as_deref(), Some("REFERENCE"));
        assert_eq!(response.connection, connection);
        let saved = std::fs::read_to_string(configuration.path())
            .expect("the saved configuration should be readable");
        assert!(saved.contains("[kitchen]"));
        assert!(saved.contains("host = \"10.42.0.71\""));
        assert!(saved.contains("port = 9200"));
    }

    #[test]
    fn add_returns_every_saved_usb_connection_fact() {
        let configuration = temporary_configuration("typed-usb-add", "");
        let connection = usb_connection(Some("B120300001"), 0x02, Some(0x83));

        let response = execute(
            Request::new(
                Some(configuration.path().to_owned()),
                "counter".to_owned(),
                Some("NT-5890K".to_owned()),
                connection,
            )
            .expect("valid USB registration facts should form an add request"),
        )
        .expect("the resolved USB request should be saved");

        assert_eq!(response.config_path, configuration.path());
        assert_eq!(response.printer_name, "counter");
        assert_eq!(response.profile.as_deref(), Some("NT-5890K"));
        assert!(matches!(
            response.connection,
            Connection::Usb {
                vendor_id: 0x0416,
                product_id: 0x5011,
                serial_number: Some(ref serial_number),
                interface_number: 0,
                out_endpoint: 0x02,
                in_endpoint: Some(0x83),
            } if serial_number == "B120300001"
        ));
    }

    fn network_connection() -> Connection {
        Connection::Network {
            host: "10.42.0.71".to_owned(),
            port: 9200,
        }
    }

    fn usb_connection(
        serial_number: Option<&str>,
        out_endpoint: u8,
        in_endpoint: Option<u8>,
    ) -> Connection {
        Connection::Usb {
            vendor_id: 0x0416,
            product_id: 0x5011,
            serial_number: serial_number.map(str::to_owned),
            interface_number: 0,
            out_endpoint,
            in_endpoint,
        }
    }
}
