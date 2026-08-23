use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::features::printers::list::{self, ConnectionFacts, Printer};
use crate::features::printers::{Availability, Transport};
use crate::profiles;

use super::ApiState;
use super::error::ApiFailure;

pub(super) fn router() -> Router<ApiState> {
    Router::new().route("/printers", get(list_printers))
}

#[derive(Serialize)]
pub(super) struct PrinterResponse {
    /// The `printers.toml` key. `/print` looks a printer up by this exact
    /// string, so it is also what `name` carries.
    id: String,
    name: String,
    transport: &'static str,
    profile: Option<String>,
    status: &'static str,
    device: DeviceResponse,
}

/// M7's device identity: stable across browsers, profiles and reinstalls,
/// because it describes the hardware rather than this machine's configuration.
#[derive(Serialize)]
#[serde(untagged)]
enum DeviceResponse {
    #[serde(rename_all = "camelCase")]
    Usb {
        usb_vendor_id: u16,
        usb_product_id: u16,
        #[serde(skip_serializing_if = "Option::is_none")]
        usb_serial: Option<String>,
    },
    Tcp {
        host: String,
        port: u16,
    },
}

impl From<Printer> for PrinterResponse {
    fn from(printer: Printer) -> Self {
        Self {
            id: printer.name.clone(),
            name: printer.name,
            transport: match printer.transport {
                Transport::Usb => "usb",
                Transport::Network => "tcp",
            },
            profile: canonical_profile(printer.profile.as_deref()),
            status: match printer.availability {
                Availability::Connected => "ready",
                Availability::Unavailable => "unavailable",
            },
            device: match printer.connection {
                ConnectionFacts::Usb(connection) => DeviceResponse::Usb {
                    usb_vendor_id: connection.vendor_id,
                    usb_product_id: connection.product_id,
                    usb_serial: connection.serial_number,
                },
                ConnectionFacts::Network(connection) => DeviceResponse::Tcp {
                    host: connection.host,
                    port: connection.port,
                },
            },
        }
    }
}

/// The catalog's own id for a configured profile, or `None` when the configured
/// value resolves to nothing.
///
/// `printers.toml` holds whatever the operator typed and the inventory copies
/// it through unchecked, so without this the API can advertise a profile that
/// does not exist — which is exactly what broke HTML rendering for every job
/// until the server began repairing it at registration.
pub(super) fn canonical_profile(configured: Option<&str>) -> Option<String> {
    profiles::load(configured?)
        .ok()
        .map(|profile| profile.id.clone())
}

pub(super) async fn load_printers(state: &ApiState) -> Result<Vec<Printer>, ApiFailure> {
    list::execute_with_observer(
        list::Request {
            config: state.config.clone(),
            transport: None,
        },
        |_| {},
    )
    .await
    .map(|response| response.printers)
    .map_err(|_| ApiFailure::printer_inventory_failure())
}

async fn list_printers(
    State(state): State<ApiState>,
) -> Result<Json<Vec<PrinterResponse>>, ApiFailure> {
    let printers = load_printers(&state).await?;
    Ok(Json(
        printers.into_iter().map(PrinterResponse::from).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::{PrinterResponse, canonical_profile};
    use crate::features::printers::list::{
        ConnectionFacts, NetworkConnectionFacts, Printer, UsbConnectionFacts,
    };
    use crate::features::printers::{Availability, Transport};

    fn usb_printer() -> Printer {
        Printer {
            name: "counter".to_owned(),
            transport: Transport::Usb,
            availability: Availability::Connected,
            profile: Some("TM-T88II".to_owned()),
            connection: ConnectionFacts::Usb(UsbConnectionFacts {
                vendor_id: 0x04b8,
                product_id: 0x0202,
                bus: Some("003".to_owned()),
                address: Some(60),
                manufacturer: None,
                product: None,
                serial_number: Some("B120300001".to_owned()),
                interface_number: 0,
                out_endpoints: vec![0x01],
                in_endpoints: Vec::new(),
            }),
        }
    }

    #[test]
    fn a_usb_printer_carries_vendor_product_and_serial() {
        // M7: a shop with three tills must bill once. The registry key is
        // per-configuration, so identity has to come from the device.
        let response = PrinterResponse::from(usb_printer());
        let json = serde_json::to_value(&response).expect("the printer should serialize");

        assert_eq!(json["transport"], "usb");
        assert_eq!(json["status"], "ready");
        assert_eq!(json["device"]["usbVendorId"], 0x04b8);
        assert_eq!(json["device"]["usbProductId"], 0x0202);
        assert_eq!(json["device"]["usbSerial"], "B120300001");
    }

    #[test]
    fn a_usb_printer_without_a_serial_omits_it_rather_than_sending_null() {
        let mut printer = usb_printer();
        let ConnectionFacts::Usb(connection) = &mut printer.connection else {
            panic!("the fixture is USB");
        };
        connection.serial_number = None;

        let json =
            serde_json::to_value(PrinterResponse::from(printer)).expect("it should serialize");

        assert!(json["device"].get("usbSerial").is_none());
        assert_eq!(json["device"]["usbVendorId"], 0x04b8);
    }

    #[test]
    fn a_network_printer_is_reported_as_tcp_with_its_endpoint() {
        // "network" is what escpost calls it internally and what the viewer API
        // emits; this surface speaks the spec's vocabulary, where the
        // capability list is ["usb", "tcp"].
        let printer = Printer {
            name: "kitchen".to_owned(),
            transport: Transport::Network,
            availability: Availability::Unavailable,
            profile: None,
            connection: ConnectionFacts::Network(NetworkConnectionFacts {
                host: "192.168.1.50".to_owned(),
                port: 9100,
            }),
        };

        let json =
            serde_json::to_value(PrinterResponse::from(printer)).expect("it should serialize");

        assert_eq!(json["transport"], "tcp");
        assert_eq!(json["status"], "unavailable");
        assert_eq!(json["device"]["host"], "192.168.1.50");
        assert_eq!(json["device"]["port"], 9100);
    }

    #[test]
    fn the_id_and_the_name_are_both_the_configuration_key() {
        // The extension resolves a requested name to `id` and then sends that
        // `id` back as /print's `printer`. If `id` were a slug it would not
        // resolve against printers.toml, so both fields carry the key.
        let json = serde_json::to_value(PrinterResponse::from(usb_printer()))
            .expect("it should serialize");
        assert_eq!(json["id"], "counter");
        assert_eq!(json["name"], "counter");
    }

    #[test]
    fn a_profile_in_the_catalog_is_reported_by_its_canonical_id() {
        assert_eq!(
            canonical_profile(Some("TM-T88II")),
            Some("TM-T88II".to_owned())
        );
    }

    #[test]
    fn a_profile_that_is_not_in_the_catalog_is_reported_as_absent() {
        // The live bug this fixes: the stub advertised "tm-t88", which is in no
        // catalog, and every HTML print failed until the server repaired it at
        // registration. Advertising an unresolvable profile as if it were real
        // is worse than admitting there is none.
        assert_eq!(canonical_profile(Some("tm-t88")), None);
        assert_eq!(canonical_profile(Some("")), None);
        assert_eq!(canonical_profile(None), None);
    }
}
