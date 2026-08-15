//! Typed operations for printer inventory, discovery, registration, and permissions.

pub(crate) mod add;
pub(crate) mod cli;
pub(crate) mod discover;
mod inventory;
pub(crate) mod list;
#[cfg(test)]
mod test_support;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Transport {
    Usb,
    Network,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Availability {
    Connected,
    Unavailable,
}

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
