//! Select Windows' existing USB printer class driver for a configured USB target.

use std::io;
use std::path::PathBuf;

use escpost_usbprint::{PrinterInterface, Writer};
use nusb::descriptors::{ConfigurationDescriptor, TransferType};
use nusb::transfer::Direction;

use super::{USB_TRANSFER_TIMEOUT, UsbTarget};
use crate::application::{self, ApplicationError};

pub(super) fn open(
    info: &nusb::DeviceInfo,
    device: &nusb::Device,
    target: &UsbTarget,
) -> application::Result<Option<Writer>> {
    let paths = escpost_usbprint::interfaces(info.instance_id())
        .map_err(|source| failure("enumeration", source))?;
    if paths.is_empty() {
        // A device already using WinUSB continues through nusb. There is no
        // driver installation, replacement, or fallback after a transfer starts.
        return Ok(None);
    }
    let config = device.active_configuration().map_err(|source| {
        ApplicationError::InspectUsbConfiguration {
            vendor_id: target.vendor_id,
            product_id: target.product_id,
            source,
        }
    })?;
    let single_interface = config.interfaces().count() == 1;
    let path = select_path(paths, target.interface, single_interface)
        .map_err(|source| failure("interface selection", source))?;
    let Some(path) = path else {
        return Ok(None);
    };
    validate_endpoint(&config, target).map_err(|source| failure("endpoint selection", source))?;
    Writer::open(&path, USB_TRANSFER_TIMEOUT)
        .map(Some)
        .map_err(|source| failure("open", source))
}

fn select_path(
    paths: Vec<PrinterInterface>,
    interface: u8,
    single_interface: bool,
) -> io::Result<Option<PathBuf>> {
    if !single_interface && paths.iter().any(|path| path.number.is_none()) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "USBPRINT is bound to a multi-interface device without a distinct printer function",
        ));
    }
    let mut matches = paths
        .into_iter()
        .filter(|path| path.number.is_none_or(|number| number == interface));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "multiple USBPRINT paths match the configured interface",
        ));
    }
    Ok(first.map(|path| path.path))
}

fn validate_endpoint(config: &ConfigurationDescriptor<'_>, target: &UsbTarget) -> io::Result<()> {
    let interface = config
        .interface_alt_settings()
        .find(|interface| {
            interface.interface_number() == target.interface && interface.alternate_setting() == 0
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "configured USB interface is absent",
            )
        })?;
    let endpoints: Vec<_> = interface
        .endpoints()
        .filter(|endpoint| {
            endpoint.direction() == Direction::Out && endpoint.transfer_type() == TransferType::Bulk
        })
        .map(|endpoint| endpoint.address())
        .collect();
    // USBPRINT chooses the bulk pipe itself. Do not silently ignore a stale
    // configured endpoint or promise to choose between multiple output pipes.
    if interface.class() != 7 || endpoints != [target.out_endpoint] {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "configured endpoint is not the printer interface's single bulk OUT endpoint",
        ));
    }
    Ok(())
}

fn failure(operation: &'static str, source: io::Error) -> ApplicationError {
    ApplicationError::WindowsUsbPrinter { operation, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(number: Option<u8>, value: &str) -> PrinterInterface {
        PrinterInterface {
            number,
            path: value.into(),
        }
    }

    #[test]
    fn selects_only_the_configured_composite_function_and_rejects_ambiguity() {
        assert_eq!(
            select_path(
                vec![path(Some(0), "other"), path(Some(2), "printer")],
                2,
                false
            )
            .unwrap(),
            Some("printer".into())
        );
        assert!(
            select_path(vec![path(Some(0), "other")], 2, false)
                .unwrap()
                .is_none()
        );
        assert!(select_path(vec![path(Some(2), "a"), path(Some(2), "b")], 2, false).is_err());
        assert!(select_path(vec![path(None, "ambiguous")], 2, false).is_err());
        assert_eq!(
            select_path(vec![path(None, "single")], 0, true).unwrap(),
            Some("single".into())
        );
    }

    #[test]
    fn stale_endpoint_or_interface_is_rejected_before_any_write() {
        let bytes = [
            9, 2, 25, 0, 1, 1, 0, 0x80, 50, 9, 4, 0, 0, 1, 7, 1, 2, 0, 7, 5, 3, 2, 64, 0, 0,
        ];
        let config = ConfigurationDescriptor::new(&bytes).unwrap();
        let mut target = UsbTarget {
            vendor_id: 0x0416,
            product_id: 0x5011,
            serial_number: None,
            interface: 0,
            out_endpoint: 3,
        };
        validate_endpoint(&config, &target).unwrap();
        target.out_endpoint = 1;
        assert!(validate_endpoint(&config, &target).is_err());
        target.out_endpoint = 3;
        target.interface = 2;
        assert!(validate_endpoint(&config, &target).is_err());
    }
}
