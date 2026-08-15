//! Test fixtures shared by several `printers` submodules' own test modules:
//! a scriptable `UsbInventory` double and USB/network device builders.

use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::inventory::{UsbDeviceIdentity, UsbEnumeration, UsbInventory, UsbPrinter};
use crate::application;
use crate::discovery::DiscoveredHost;

#[derive(Default)]
pub(super) struct FixedInventory {
    pub(super) printers: Vec<UsbPrinter>,
}

impl UsbInventory for FixedInventory {
    fn list(&mut self) -> application::Result<Vec<UsbPrinter>> {
        Ok(self.printers.clone())
    }

    fn identities(&mut self) -> application::Result<Vec<UsbDeviceIdentity>> {
        Ok(self.printers.iter().map(usb_printer_identity).collect())
    }
}

pub(super) struct TolerantInventory {
    pub(super) enumeration: Option<UsbEnumeration>,
}

impl UsbInventory for TolerantInventory {
    fn list(&mut self) -> application::Result<Vec<UsbPrinter>> {
        Ok(Vec::new())
    }

    fn identities(&mut self) -> application::Result<Vec<UsbDeviceIdentity>> {
        Ok(Vec::new())
    }

    fn list_tolerant(&mut self) -> application::Result<UsbEnumeration> {
        Ok(self
            .enumeration
            .take()
            .expect("the tolerant fixture should be enumerated once"))
    }
}

/// Derive a metadata-only device identity from a `UsbPrinter` test
/// fixture, the same fields `NusbInventory::identities` would read from
/// `nusb::DeviceInfo` without opening the device. Test doubles use this
/// so one fixture can drive both the open-based `list()`/`list_tolerant()`
/// paths (discover, add) and the metadata-only `identities()` path
/// (list) without keeping two copies of the same descriptor in sync.
pub(super) fn usb_printer_identity(printer: &UsbPrinter) -> UsbDeviceIdentity {
    UsbDeviceIdentity {
        vendor_id: printer.vendor_id,
        product_id: printer.product_id,
        bus: printer.bus.clone(),
        address: printer.address,
        manufacturer: printer.manufacturer.clone(),
        product: printer.product.clone(),
        serial_number: printer.serial_number.clone(),
    }
}

pub(super) fn netum_usb_printer(out_endpoints: Vec<u8>, in_endpoints: Vec<u8>) -> UsbPrinter {
    UsbPrinter {
        vendor_id: 0x0416,
        product_id: 0x5011,
        bus: "003".to_owned(),
        address: 60,
        manufacturer: Some("YICHIP3121".to_owned()),
        product: Some("USB Portable Printer".to_owned()),
        serial_number: Some("B120300001".to_owned()),
        interface_number: 0,
        out_endpoints,
        in_endpoints,
    }
}

pub(super) fn discovered(address: [u8; 4], port: u16) -> DiscoveredHost {
    DiscoveredHost {
        address: Ipv4Addr::from(address),
        port,
        interface: Some("enx0".to_owned()),
    }
}

pub(super) struct TemporaryConfiguration {
    directory: PathBuf,
    path: PathBuf,
}

impl TemporaryConfiguration {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryConfiguration {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

pub(super) fn temporary_configuration(case: &str, content: &str) -> TemporaryConfiguration {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock should be after the Unix epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "escpost-printers-{case}-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("the test directory should be creatable");
    let path = directory.join("printers.toml");
    if !content.is_empty() {
        fs::write(&path, content).expect("the fixture configuration should be writable");
    }
    TemporaryConfiguration { directory, path }
}
