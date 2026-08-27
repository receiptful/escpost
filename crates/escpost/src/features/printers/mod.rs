//! Typed operations for printer inventory, discovery, registration, and permissions.

pub(crate) mod add;
pub(crate) mod cli;
pub(crate) mod discover;
pub(crate) mod http;
mod inventory;
pub(crate) mod list;
pub(crate) mod monitor;
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
