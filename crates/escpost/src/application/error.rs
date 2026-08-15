use std::path::PathBuf;

use thiserror::Error;

/// A factual failure from an application operation or one of its dependencies.
///
/// This type deliberately contains no terminal instructions, command examples,
/// HTTP status choices, or other adapter presentation. Adapters add their own
/// recovery guidance when they translate an application failure for a user.
#[derive(Debug, Error)]
pub(crate) enum ApplicationError {
    #[error("could not load the embedded printer profiles: {0}")]
    LoadProfiles(String),

    #[error("unknown printer profile {0:?}")]
    UnknownProfile(String),

    #[error("could not read ESC/POS input {path}: {source}")]
    ReadInput {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not read ESC/POS input from stdin: {0}")]
    ReadStdin(std::io::Error),

    #[error("directory is not a recognized ESCPost case: {0}")]
    UnrecognizedDirectory(PathBuf),

    #[error("invalid case manifest {path}: {message}")]
    InvalidCaseManifest { path: PathBuf, message: String },

    #[error("unsupported case schema version {0}")]
    UnsupportedCaseSchema(u32),

    #[error("case field {0} must not be empty")]
    EmptyCaseField(&'static str),

    #[error("hexadecimal input is not UTF-8: {0}")]
    InvalidHexEncoding(#[from] std::str::Utf8Error),

    #[error("invalid hexadecimal byte {token:?} at token {position}")]
    InvalidHexByte { token: String, position: usize },

    #[error("could not render ESC/POS input: {0}")]
    Render(String),

    #[error("single-PNG output requires exactly one sheet, but rendering produced {0}")]
    MultipleSheets(usize),

    #[error("sheet {requested} does not exist; rendering produced {available} sheet(s)")]
    SheetOutOfRange { requested: usize, available: usize },

    #[error("could not write PNG output {path}: {source}")]
    WriteOutput {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not create output directory {path}: {source}")]
    CreateOutputDirectory {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not serialize the output manifest: {0}")]
    SerializeManifest(#[from] serde_json::Error),

    #[error("could not enumerate USB devices: {0}")]
    EnumerateUsb(nusb::Error),

    #[error("could not enumerate network interfaces: {0}")]
    EnumerateNetworkInterfaces(std::io::Error),

    #[error(
        "no directly connected IPv4 network is small enough to scan automatically (at most /24)"
    )]
    NoDiscoverableSubnets,

    #[error("no USB device matches vendor {vendor_id:#06x} and product {product_id:#06x}")]
    UsbDeviceNotFound { vendor_id: u16, product_id: u16 },

    #[error(
        "{count} USB devices match vendor {vendor_id:#06x} and product {product_id:#06x}; refusing to choose one implicitly"
    )]
    AmbiguousUsbDevices {
        vendor_id: u16,
        product_id: u16,
        count: usize,
    },

    #[error("USB OUT endpoint must be between 0x01 and 0x0f, got {0:#04x}")]
    InvalidUsbOutEndpoint(u8),

    #[error("USB IN endpoint must be between 0x81 and 0x8f, got {0:#04x}")]
    InvalidUsbInEndpoint(u8),

    #[error("could not open USB device {vendor_id:#06x}:{product_id:#06x}: {source}")]
    OpenUsbDevice {
        vendor_id: u16,
        product_id: u16,
        source: std::io::Error,
    },

    #[error(
        "could not inspect the active configuration of USB device {vendor_id:#06x}:{product_id:#06x}: {source}"
    )]
    InspectUsbConfiguration {
        vendor_id: u16,
        product_id: u16,
        source: nusb::ActiveConfigurationError,
    },

    #[error("could not detach and claim USB interface {interface}: {source}")]
    ClaimUsbInterface {
        interface: u8,
        source: std::io::Error,
    },

    #[error(
        "could not open bulk OUT endpoint {endpoint:#04x} on USB interface {interface}: {source}"
    )]
    OpenUsbOutEndpoint {
        interface: u8,
        endpoint: u8,
        source: std::io::Error,
    },

    #[error("could not write ESC/POS bytes to USB endpoint {endpoint:#04x}: {source}")]
    WriteUsb {
        endpoint: u8,
        source: std::io::Error,
    },

    #[error("could not finish the USB write on endpoint {endpoint:#04x}: {source}")]
    FlushUsb {
        endpoint: u8,
        source: std::io::Error,
    },

    #[error("printer {0:?} is not configured")]
    UnknownConfiguredPrinter(String),

    #[error("timed out while connecting to network printer {0}")]
    ConnectNetworkPrinterTimeout(String),

    #[error("could not connect to network printer {target}: {source}")]
    ConnectNetworkPrinter {
        target: String,
        source: std::io::Error,
    },

    #[error("timed out while writing to network printer {0}")]
    WriteNetworkPrinterTimeout(String),

    #[error("could not write to network printer {target}: {source}")]
    WriteNetworkPrinter {
        target: String,
        source: std::io::Error,
    },

    #[error("could not read printer configuration {}: {source}", crate::configuration::display_path(.path.as_path()))]
    ReadPrinterConfiguration {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid printer configuration {}: {message}", crate::configuration::display_path(.path.as_path()))]
    InvalidPrinterConfiguration { path: PathBuf, message: String },

    #[error("printer name must not be blank")]
    BlankPrinterName,

    #[error("network printer host must not be blank")]
    BlankPrinterHost,

    #[error("printer port must be between 1 and 65535")]
    InvalidPrinterPort,

    #[error("printer profile must not be blank")]
    BlankPrinterProfile,

    #[error("USB serial number must not be blank")]
    BlankUsbSerialNumber,

    #[error("printer {0:?} is already configured")]
    PrinterAlreadyConfigured(String),

    #[error("could not create printer configuration directory {}: {source}", crate::configuration::display_path(.path.as_path()))]
    CreatePrinterConfigurationDirectory {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("could not serialize printer configuration: {0}")]
    SerializePrinterConfiguration(String),

    #[error("could not write printer configuration {}: {source}", crate::configuration::display_path(.path.as_path()))]
    WritePrinterConfiguration {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("the operating system did not provide a user configuration directory")]
    NoUserConfigDirectory,

    #[error("could not inspect watched source {path}: {source}")]
    InspectWatchedSource {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl ApplicationError {
    pub(crate) fn is_permission_denied_usb_open(&self) -> bool {
        match self {
            Self::OpenUsbDevice { source, .. }
            | Self::ClaimUsbInterface { source, .. }
            | Self::OpenUsbOutEndpoint { source, .. } => {
                source.kind() == std::io::ErrorKind::PermissionDenied
            }
            _ => false,
        }
    }
}
