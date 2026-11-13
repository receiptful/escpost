//! Windows USB printer class driver access, without a spooler or replacement driver.
//!
//! The caller selects a unique physical USB device and validates its printer
//! interface/endpoint. This crate maps that device's Windows instance ID to a
//! live USBPRINT interface and provides deadline-limited writes. A failed write
//! poisons the writer; the caller must not replay a potentially printed prefix.

#[cfg(windows)]
#[allow(unsafe_code)]
mod native;

#[cfg(windows)]
pub use native::{PrinterInterface, Writer, interfaces};

#[cfg(any(windows, test))]
fn composite_interface(instance_id: &str) -> Option<u8> {
    let hardware = instance_id.split('\\').nth(1)?.to_ascii_uppercase();
    let (_, number) = hardware.rsplit_once("&MI_")?;
    if number.len() != 2 {
        return None;
    }
    u8::from_str_radix(number, 16).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn composite_matching_uses_the_interface_id_not_the_serial_or_port() {
        assert_eq!(
            super::composite_interface(r"USB\VID_0416&PID_5011&MI_02\7&123&0&0002"),
            Some(2)
        );
        assert_eq!(
            super::composite_interface(r"usb\vid_0416&pid_5011&mi_0a\serial"),
            Some(10)
        );
        assert_eq!(
            super::composite_interface(r"USB\VID_0416&PID_5011\serial&MI_02"),
            None
        );
        assert_eq!(
            super::composite_interface(r"USB\VID_0416&PID_5011&MI_002\serial"),
            None
        );
    }
}
