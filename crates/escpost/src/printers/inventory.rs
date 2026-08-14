//! USB device access and USB-identity-to-configuration matching, shared by
//! `printers list`, `printers add`, and `printers discover`.

use crate::configuration::{ConfiguredUsbPrinter, PrinterConfiguration};
use crate::error::CliError;
use nusb::MaybeFuture;
use nusb::descriptors::{ConfigurationDescriptor, TransferType};
use nusb::transfer::Direction;

const USB_CLASS_PRINTER: u8 = 0x07;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UsbPrinter {
    pub(super) vendor_id: u16,
    pub(super) product_id: u16,
    pub(super) bus: String,
    pub(super) address: u8,
    pub(super) manufacturer: Option<String>,
    pub(super) product: Option<String>,
    pub(super) serial_number: Option<String>,
    pub(super) interface_number: u8,
    pub(super) out_endpoints: Vec<u8>,
    pub(super) in_endpoints: Vec<u8>,
}
/// OS-reported identity of a printer-class USB device, gathered without ever
/// opening it (see `UsbInventory::identities`). `printers list` uses this
/// alone to decide whether a saved USB printer is connected: interface and
/// endpoint routing are not available at this level, so a matched entry's
/// display block sources those from the saved configuration instead (see
/// `connected_usb_printer`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UsbDeviceIdentity {
    pub(super) vendor_id: u16,
    pub(super) product_id: u16,
    pub(super) bus: String,
    pub(super) address: u8,
    pub(super) manufacturer: Option<String>,
    pub(super) product: Option<String>,
    pub(super) serial_number: Option<String>,
}
/// Best-effort USB enumeration for `printers discover`: printers found so
/// far, plus a warning line for each device that could not be opened or
/// whose active configuration could not be inspected. A device-level
/// failure never aborts the rest of the sweep, the same way the network
/// sweep silently skips unreachable hosts. `permission_denied` is set when
/// at least one of those per-device failures was
/// `CliError::is_permission_denied_usb_open` — the same predicate the
/// top-level fatal-error print in `lib.rs` uses — computed once here, where
/// the structured `CliError` is still available, rather than re-derived
/// from the formatted `warnings` strings by the caller.
pub(super) struct UsbEnumeration {
    pub(super) printers: Vec<UsbPrinter>,
    pub(super) warnings: Vec<String>,
    pub(super) permission_denied: bool,
}
#[derive(Debug, PartialEq, Eq)]
struct UsbPrinterInterface {
    interface_number: u8,
    out_endpoints: Vec<u8>,
    in_endpoints: Vec<u8>,
}
pub(super) struct ConnectedUsbPrinter {
    pub(super) printer: UsbPrinter,
    pub(super) configuration_index: Option<usize>,
}
/// The result of matching `printers list`'s metadata-only USB identities
/// against the saved configuration (see `merge_usb_identities`). Unlike
/// `printers discover`'s `ConnectedUsbPrinter`, an identity matching no
/// saved printer is simply dropped rather than kept with `configuration_index:
/// None`: `list` never shows a connected-but-unconfigured USB device, so
/// every entry here is already known to belong to one configuration index.
pub(super) struct MergedUsbIdentities {
    pub(super) connected: Vec<ConnectedUsbEntry>,
    pub(super) unavailable_configuration_indexes: Vec<usize>,
}
pub(super) struct ConnectedUsbEntry {
    pub(super) printer: UsbPrinter,
    pub(super) configuration_index: usize,
}
pub(super) trait UsbInventory {
    fn list(&mut self) -> Result<Vec<UsbPrinter>, CliError>;

    /// Metadata-only USB presence check for `printers list`: the OS-reported
    /// identity (vendor, product, serial, live bus/address, and
    /// manufacturer/product strings) of every printer-class device, without
    /// ever opening one. There is no per-device failure mode here — nothing
    /// about an individual device is opened or inspected — so, unlike
    /// `list_tolerant`, total enumeration failure is the only error this can
    /// return.
    fn identities(&mut self) -> Result<Vec<UsbDeviceIdentity>, CliError>;

    /// Best-effort enumeration for `printers discover`: a device that fails
    /// to open or whose active configuration cannot be inspected is skipped
    /// with a warning instead of aborting the whole enumeration, mirroring
    /// the network sweep's own tolerance of unreachable hosts. Total
    /// enumeration failure (the initial USB device listing itself) still
    /// propagates as an error. The default forwards to the strict `list()`
    /// with no warnings, since only the real USB backend can fail on
    /// individual devices; a test double that needs to exercise a partial
    /// failure overrides this directly.
    fn list_tolerant(&mut self) -> Result<UsbEnumeration, CliError> {
        Ok(UsbEnumeration {
            printers: self.list()?,
            warnings: Vec::new(),
            permission_denied: false,
        })
    }
}
/// Sort connected USB printers by stable location. Both `printers list`'s
/// merge and `printers discover`'s classification rely on this order to make
/// first-match-wins configuration assignment deterministic across runs,
/// regardless of the order the operating system enumerates devices.
pub(super) fn sort_by_usb_location(printers: &mut [UsbPrinter]) {
    printers.sort_by(|left, right| {
        (
            &left.bus,
            left.address,
            left.interface_number,
            left.vendor_id,
            left.product_id,
        )
            .cmp(&(
                &right.bus,
                right.address,
                right.interface_number,
                right.vendor_id,
                right.product_id,
            ))
    });
}
/// Match each connected USB printer against at most one configured identity,
/// first-match-wins in `printers` order. This keeps one saved alias from
/// naming several identical connected interfaces when the configuration has
/// no serial number. `printers` must already be sorted by stable USB
/// location (`sort_by_usb_location`) so the match is deterministic across
/// runs. Returns the connected printers alongside which configuration
/// indexes were claimed, so callers can also report unclaimed ones.
pub(super) fn classify_usb_printers(
    printers: Vec<UsbPrinter>,
    configuration: &PrinterConfiguration,
) -> (Vec<ConnectedUsbPrinter>, Vec<bool>) {
    let mut matched_configurations = vec![false; configuration.usb_printers().len()];
    let mut connected = Vec::with_capacity(printers.len());
    for printer in printers {
        let matching_configurations = configuration
            .usb_printers()
            .iter()
            .enumerate()
            .filter(|(_, configured)| configuration_matches(&printer, configured))
            .collect::<Vec<_>>();
        let primary_configuration = matching_configurations
            .iter()
            .filter(|(index, _)| !matched_configurations[*index])
            .min_by(|(_, left), (_, right)| compare_display_names(&left.name, &right.name))
            .map(|(index, _)| *index);
        if primary_configuration.is_some() {
            for (configuration_index, _) in matching_configurations {
                matched_configurations[configuration_index] = true;
            }
        }
        connected.push(ConnectedUsbPrinter {
            printer,
            configuration_index: primary_configuration,
        });
    }
    (connected, matched_configurations)
}
/// The list-specific analogue of `configuration_matches`: whether an
/// OS-reported device identity (no interface or endpoint data available
/// without opening the device) satisfies a saved USB printer. Serial
/// semantics mirror `configuration_matches` exactly: an unset saved serial
/// matches any identity of that vendor/product, a set one requires an exact
/// match.
fn identity_matches_configuration(
    identity: &UsbDeviceIdentity,
    configured: &ConfiguredUsbPrinter,
) -> bool {
    configured.vendor_id == identity.vendor_id
        && configured.product_id == identity.product_id
        && configured
            .serial_number
            .as_ref()
            .is_none_or(|serial| identity.serial_number.as_ref() == Some(serial))
}
/// Sort device identities by stable location, the identity-level analogue of
/// `sort_by_usb_location`. `merge_usb_identities` relies on this order for
/// the same reason `classify_usb_printers` relies on `sort_by_usb_location`:
/// deterministic first-match-wins configuration assignment regardless of the
/// order the operating system enumerates devices.
fn sort_by_usb_identity_location(identities: &mut [UsbDeviceIdentity]) {
    identities.sort_by(|left, right| {
        (&left.bus, left.address, left.vendor_id, left.product_id).cmp(&(
            &right.bus,
            right.address,
            right.vendor_id,
            right.product_id,
        ))
    });
}
/// Compose the printer shown for one matched `list` entry: live location and
/// descriptor strings come from the OS-reported identity (never opened),
/// while interface and endpoint routing come from the saved configuration —
/// `list` never reads endpoints from the device itself, only from
/// `printers.toml`. The serial line prefers the identity's serial (today's
/// live value) and falls back to the configured one so an entry matched by
/// an unset configured serial still shows the connected device's own serial
/// when it has one.
fn connected_usb_printer(
    identity: UsbDeviceIdentity,
    configured: &ConfiguredUsbPrinter,
) -> UsbPrinter {
    UsbPrinter {
        vendor_id: identity.vendor_id,
        product_id: identity.product_id,
        bus: identity.bus,
        address: identity.address,
        manufacturer: identity.manufacturer,
        product: identity.product,
        serial_number: identity
            .serial_number
            .or_else(|| configured.serial_number.clone()),
        interface_number: configured.interface_number,
        out_endpoints: vec![configured.out_endpoint],
        in_endpoints: configured.in_endpoint.into_iter().collect(),
    }
}
/// The list-specific analogue of `classify_usb_printers`: match each
/// metadata-only device identity against the saved USB configuration,
/// first-match-wins by stable location exactly like that function, then
/// sort the connected results by display name. An identity matching no
/// saved configuration is dropped outright — unlike `printers discover`,
/// `list` never shows a connected-but-unconfigured USB device — and a saved
/// printer claimed by an identity that lost the first-match-wins tiebreak
/// to a sibling configuration is neither connected nor unavailable,
/// mirroring `classify_usb_printers`' own ambiguity handling.
pub(super) fn merge_usb_identities(
    mut identities: Vec<UsbDeviceIdentity>,
    configuration: &PrinterConfiguration,
) -> MergedUsbIdentities {
    sort_by_usb_identity_location(&mut identities);
    let mut matched_configurations = vec![false; configuration.usb_printers().len()];
    let mut connected = Vec::new();
    for identity in identities {
        let matching_configurations = configuration
            .usb_printers()
            .iter()
            .enumerate()
            .filter(|(_, configured)| identity_matches_configuration(&identity, configured))
            .collect::<Vec<_>>();
        let primary_configuration = matching_configurations
            .iter()
            .filter(|(index, _)| !matched_configurations[*index])
            .min_by(|(_, left), (_, right)| compare_display_names(&left.name, &right.name))
            .map(|(index, _)| *index);
        let Some(configuration_index) = primary_configuration else {
            continue;
        };
        for (index, _) in matching_configurations {
            matched_configurations[index] = true;
        }
        let printer =
            connected_usb_printer(identity, &configuration.usb_printers()[configuration_index]);
        connected.push(ConnectedUsbEntry {
            printer,
            configuration_index,
        });
    }
    connected.sort_by_cached_key(|connected| {
        let name = &configuration.usb_printers()[connected.configuration_index].name;
        (name.to_lowercase(), name.clone())
    });
    let mut unavailable_configuration_indexes = configuration
        .usb_printers()
        .iter()
        .enumerate()
        .filter(|(index, _)| !matched_configurations[*index])
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    unavailable_configuration_indexes.sort_by(|left, right| {
        compare_display_names(
            &configuration.usb_printers()[*left].name,
            &configuration.usb_printers()[*right].name,
        )
    });

    MergedUsbIdentities {
        connected,
        unavailable_configuration_indexes,
    }
}
pub(super) struct NusbInventory;

impl UsbInventory for NusbInventory {
    fn list(&mut self) -> Result<Vec<UsbPrinter>, CliError> {
        let devices = nusb::list_devices()
            .wait()
            .map_err(CliError::EnumerateUsb)?;
        let mut printers = Vec::new();

        // Filter with operating-system metadata first. Listing should never
        // open unrelated USB devices merely to find their interface classes.
        for device_info in devices.filter(is_printer_device) {
            printers.extend(usb_printers_for_device(&device_info)?);
        }

        Ok(printers)
    }

    fn list_tolerant(&mut self) -> Result<UsbEnumeration, CliError> {
        let devices = nusb::list_devices()
            .wait()
            .map_err(CliError::EnumerateUsb)?;
        let mut printers = Vec::new();
        let mut warnings = Vec::new();
        let mut permission_denied = false;

        for device_info in devices.filter(is_printer_device) {
            match usb_printers_for_device(&device_info) {
                Ok(device_printers) => printers.extend(device_printers),
                Err(error) => {
                    permission_denied |= error.is_permission_denied_usb_open();
                    warnings.push(describe_usb_enumeration_failure(&error));
                }
            }
        }

        Ok(UsbEnumeration {
            printers,
            warnings,
            permission_denied,
        })
    }

    fn identities(&mut self) -> Result<Vec<UsbDeviceIdentity>, CliError> {
        let devices = nusb::list_devices()
            .wait()
            .map_err(CliError::EnumerateUsb)?;

        // Operating-system device metadata only: no `.open()` anywhere in
        // this path, so `printers list` cannot fail with a permission error
        // the way opening a device for `discover` or `add` can.
        Ok(devices
            .filter(is_printer_device)
            .map(|device_info| UsbDeviceIdentity {
                vendor_id: device_info.vendor_id(),
                product_id: device_info.product_id(),
                bus: device_info.bus_id().to_owned(),
                address: device_info.device_address(),
                manufacturer: device_info.manufacturer_string().map(str::to_owned),
                product: device_info.product_string().map(str::to_owned),
                serial_number: device_info.serial_number().map(str::to_owned),
            })
            .collect())
    }
}
/// Open one USB device and collect the printer-class interfaces it exposes.
/// Shared by `list()`'s strict enumeration (used by `printers list` and
/// `printers add`, where a device failure aborts the whole command) and
/// `list_tolerant()`'s best-effort enumeration (used by `printers discover`,
/// where a device failure becomes a warning and enumeration continues).
fn usb_printers_for_device(device_info: &nusb::DeviceInfo) -> Result<Vec<UsbPrinter>, CliError> {
    let device = device_info
        .open()
        .wait()
        .map_err(|source| CliError::OpenUsbDevice {
            vendor_id: device_info.vendor_id(),
            product_id: device_info.product_id(),
            source,
        })?;
    let configuration =
        device
            .active_configuration()
            .map_err(|source| CliError::InspectUsbConfiguration {
                vendor_id: device_info.vendor_id(),
                product_id: device_info.product_id(),
                source,
            })?;

    Ok(printer_interfaces(configuration)
        .into_iter()
        .map(|interface| UsbPrinter {
            vendor_id: device_info.vendor_id(),
            product_id: device_info.product_id(),
            bus: device_info.bus_id().to_owned(),
            address: device_info.device_address(),
            manufacturer: device_info.manufacturer_string().map(str::to_owned),
            product: device_info.product_string().map(str::to_owned),
            serial_number: device_info.serial_number().map(str::to_owned),
            interface_number: interface.interface_number,
            out_endpoints: interface.out_endpoints,
            in_endpoints: interface.in_endpoints,
        })
        .collect())
}
/// Render a USB enumeration failure gathered by `list_tolerant` as a warning
/// line for `printers discover`, using the same bare `vendor:product` hex
/// notation as discover's own `usb:` coordinate line. `CliError`'s Display
/// uses a `0x`-prefixed form instead, which stays unchanged for the fatal
/// case where `list()` itself propagates the error (`printers list`,
/// `printers add`).
pub(super) fn describe_usb_enumeration_failure(error: &CliError) -> String {
    match error {
        CliError::OpenUsbDevice {
            vendor_id,
            product_id,
            source,
        } => format!("could not open USB device {vendor_id:04x}:{product_id:04x}: {source}"),
        CliError::InspectUsbConfiguration {
            vendor_id,
            product_id,
            source,
        } => format!(
            "could not inspect the active configuration of USB device {vendor_id:04x}:{product_id:04x}: {source}"
        ),
        other => other.to_string(),
    }
}
fn is_printer_device(device: &nusb::DeviceInfo) -> bool {
    device.class() == USB_CLASS_PRINTER
        || device
            .interfaces()
            .any(|interface| interface.class() == USB_CLASS_PRINTER)
}
pub(super) fn configuration_matches(
    printer: &UsbPrinter,
    configured: &ConfiguredUsbPrinter,
) -> bool {
    configured.vendor_id == printer.vendor_id
        && configured.product_id == printer.product_id
        && configured.interface_number == printer.interface_number
        && printer.out_endpoints.contains(&configured.out_endpoint)
        && configured
            .serial_number
            .as_ref()
            .is_none_or(|serial| printer.serial_number.as_ref() == Some(serial))
}
fn compare_display_names(left: &str, right: &str) -> std::cmp::Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}
fn printer_interfaces(configuration: ConfigurationDescriptor<'_>) -> Vec<UsbPrinterInterface> {
    configuration
        .interface_alt_settings()
        .filter(|interface| {
            // The print command does not change alternate settings. Only show
            // endpoints that will exist immediately after claiming an
            // interface in its standard alternate setting.
            interface.class() == USB_CLASS_PRINTER && interface.alternate_setting() == 0
        })
        .filter_map(|interface| {
            let mut out_endpoints = Vec::new();
            let mut in_endpoints = Vec::new();
            for endpoint in interface
                .endpoints()
                .filter(|endpoint| endpoint.transfer_type() == TransferType::Bulk)
            {
                match endpoint.direction() {
                    Direction::Out => out_endpoints.push(endpoint.address()),
                    Direction::In => in_endpoints.push(endpoint.address()),
                }
            }
            out_endpoints.sort_unstable();
            in_endpoints.sort_unstable();

            (!out_endpoints.is_empty()).then_some(UsbPrinterInterface {
                interface_number: interface.interface_number(),
                out_endpoints,
                in_endpoints,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_printer_class_bulk_endpoints_are_listed() {
        let descriptor_bytes = [
            9, 2, 55, 0, 2, 1, 0, 0x80, 50, // configuration
            9, 4, 0, 0, 3, 7, 1, 2, 0, // printer interface
            7, 5, 0x01, 2, 64, 0, 0, // bulk OUT
            7, 5, 0x81, 2, 64, 0, 0, // bulk IN
            7, 5, 0x82, 3, 8, 0, 10, // interrupt IN, not a print endpoint
            9, 4, 1, 0, 1, 0xff, 0, 0, 0, // vendor-specific interface
            7, 5, 0x02, 2, 64, 0, 0, // bulk OUT, but not printer class
        ];
        let configuration = nusb::descriptors::ConfigurationDescriptor::new(&descriptor_bytes)
            .expect("the descriptor should be valid");

        let interfaces = printer_interfaces(configuration);

        assert_eq!(
            interfaces,
            vec![UsbPrinterInterface {
                interface_number: 0,
                out_endpoints: vec![0x01],
                in_endpoints: vec![0x81],
            }]
        );
    }
}
