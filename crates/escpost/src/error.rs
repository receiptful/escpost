use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum CliError {
    #[error("could not load the embedded printer profiles: {0}")]
    LoadProfiles(String),

    #[error("printer profile is required; pass --profile REFERENCE for generic rendering")]
    MissingProfile,

    #[error("unknown printer profile {0:?}")]
    UnknownProfile(String),

    #[error("could not select a printer profile: {0}")]
    ProfilePrompt(String),

    #[error(
        "interactive selection is unavailable; run `escpost profiles list --search <text>` instead"
    )]
    InteractiveFindUnavailable,

    #[error(
        "an output destination is required; pass --output <PNG>, --output-dir <DIRECTORY>, or --web"
    )]
    MissingOutput,

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

    #[error("could not write PNG output to stdout: {0}")]
    WriteStdout(std::io::Error),

    #[error("refusing to write binary PNG data to an interactive terminal")]
    BinaryOutputToTerminal,

    #[error("PNG stdout cannot be combined with a long-running web viewer")]
    StdoutWithWeb,

    #[error("could not bind web viewer to {address}: {source}")]
    BindWeb {
        address: std::net::SocketAddr,
        source: std::io::Error,
    },

    #[error("no loopback web port from 9000 through 9099 is available")]
    NoAutomaticWebPort,

    #[error("web viewer failed: {0}")]
    ServeWeb(std::io::Error),

    #[error("could not bind RAW printer to {address}: {source}")]
    BindRawPrinter {
        address: std::net::SocketAddr,
        source: std::io::Error,
    },

    #[error("no loopback RAW printer port from 9100 through 9109 is available")]
    NoAutomaticRawPort,

    #[error("RAW printer failed: {0}")]
    ServeRawPrinter(std::io::Error),

    #[error("idle timeout must be a positive number of seconds")]
    InvalidIdleTimeout,

    #[error("watch mode requires a filesystem source, not stdin")]
    WatchStdin,

    #[error("could not enumerate USB devices: {0}")]
    EnumerateUsb(nusb::Error),

    #[error("could not enumerate network interfaces: {0}")]
    EnumerateNetworkInterfaces(std::io::Error),

    #[error(
        "no directly connected IPv4 network is small enough to scan automatically (at most /24); pass --subnet <CIDR>"
    )]
    NoDiscoverableSubnets,

    #[error("--discover is only valid for network printers")]
    DiscoverForUsbPrinter,

    #[error("--subnet, --port, and --timeout are only valid when discovering network printers")]
    NetworkScanOptionForUsbDiscovery,

    #[error("no printer is listening on port {0} in the scanned networks")]
    NoDiscoveredPrinters(u16),

    #[error(
        "several printers were discovered; choose one interactively or pass --host:\n{}",
        .0.join("\n")
    )]
    AmbiguousDiscoveredPrinters(Vec<String>),

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

    #[error("could not open USB device {vendor_id:#06x}:{product_id:#06x}: {source}")]
    OpenUsbDevice {
        vendor_id: u16,
        product_id: u16,
        source: nusb::Error,
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
    ClaimUsbInterface { interface: u8, source: nusb::Error },

    #[error(
        "could not open bulk OUT endpoint {endpoint:#04x} on USB interface {interface}: {source}"
    )]
    OpenUsbOutEndpoint {
        interface: u8,
        endpoint: u8,
        source: nusb::Error,
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

    #[error("printer is required; pass --printer <NAME>")]
    MissingPrintPrinter,

    #[error("printer {0:?} is not configured; use `escpost printers list` to see available names")]
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

    #[error("could not write command output: {0}")]
    WriteHumanOutput(std::io::Error),

    #[error("could not read printer configuration {}: {source}", crate::configuration::display_path(.path.as_path()))]
    ReadPrinterConfiguration {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid printer configuration {}: {message}", crate::configuration::display_path(.path.as_path()))]
    InvalidPrinterConfiguration { path: PathBuf, message: String },

    #[error("printer name is required")]
    MissingPrinterName,

    #[error("printer name must not be blank")]
    BlankPrinterName,

    #[error("printer transport is required")]
    MissingPrinterTransport,

    #[error(
        "USB printer registration requires an interactive terminal or explicit --vendor-id and --product-id selectors"
    )]
    UsbRegistrationRequiresInteractive,

    #[error("--vendor-id and --product-id must be given together to select a USB printer")]
    IncompleteUsbSelector,

    #[error("--vendor-id, --product-id, and --serial are only valid for USB printers")]
    UsbSelectorForNetworkPrinter,

    #[error("no connected USB printer matched the given selectors")]
    NoMatchingUsbPrinter,

    #[error(
        "several connected USB printers matched the given selectors; narrow the selection with --serial or register interactively"
    )]
    AmbiguousUsbPrinter,

    #[error("--host is only valid for network printers")]
    NetworkHostForUsbPrinter,

    #[error("--port is only valid for network printers")]
    NetworkPortForUsbPrinter,

    #[error("no unconfigured connected USB printers were found")]
    NoUnconfiguredUsbPrinters,

    #[error("network printer host is required")]
    MissingPrinterHost,

    #[error("network printer host must not be blank")]
    BlankPrinterHost,

    #[error("could not read printer information: {0}")]
    PrinterPrompt(String),

    #[error("printer port must be between 1 and 65535")]
    InvalidPrinterPort,

    #[error("printer profile must not be blank")]
    BlankPrinterProfile,

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

    #[cfg(target_os = "linux")]
    #[error("could not read existing udev rule {path}: {source}")]
    ReadUsbRulesFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[cfg(target_os = "linux")]
    #[error("could not write udev rule {path}: {source}")]
    WriteUsbRulesFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[cfg(target_os = "linux")]
    #[error(
        "udev rule {path} already exists with different content; refusing to overwrite a possibly hand-edited rule.\n--- existing {path} ---\n{existing}--- desired ---\n{desired}"
    )]
    UsbRuleDiverges {
        path: PathBuf,
        existing: String,
        desired: String,
    },

    #[cfg(target_os = "linux")]
    #[error("could not run `{command}`: {source}")]
    RunUdevadm {
        command: String,
        source: std::io::Error,
    },

    #[cfg(target_os = "linux")]
    #[error("`{command}` failed ({status}): {stderr}")]
    UdevadmFailed {
        command: String,
        status: std::process::ExitStatus,
        stderr: String,
    },
}

impl CliError {
    /// Whether this error is a USB **open**-family failure caused by an
    /// operating system permission denial (Linux's EACCES/EPERM, errno 13)
    /// — the exact condition a udev rule installed by `printers grant-usb-permissions`
    /// fixes. Covers the three points where escpost calls into nusb's
    /// blocking open/claim path and a root-owned device node surfaces as a
    /// permission error today: opening the device itself (`OpenUsbDevice`,
    /// reached by `printers add`'s USB selection and `print`'s physical
    /// send path, plus `printers discover`'s tolerant sweep, which turns it
    /// into a warning instead of a fatal error) and, only from `print`'s
    /// send path, claiming the interface (`ClaimUsbInterface`) and opening
    /// its bulk OUT endpoint (`OpenUsbOutEndpoint`). Notably *not*
    /// `printers list`: its metadata-only `identities()` path never calls
    /// `.open()` at all (see `printers::list`'s module docs), so it cannot
    /// hit this condition structurally, regardless of device permissions.
    /// Deliberately excludes `WriteUsb`/`FlushUsb`: those already had a
    /// successful open, so a permission error there would mean something
    /// changed mid-session, not the missing-udev-rule case this hint
    /// targets. Shared by two call sites:
    /// the top-level fatal-error print in `lib.rs` (any command) and
    /// `NusbInventory::list_tolerant` (`printers discover`'s per-device
    /// warnings), so there is exactly one place that knows what "permission
    /// denied" means for a `CliError`.
    pub(crate) fn is_permission_denied_usb_open(&self) -> bool {
        match self {
            CliError::OpenUsbDevice { source, .. }
            | CliError::ClaimUsbInterface { source, .. }
            | CliError::OpenUsbOutEndpoint { source, .. } => {
                source.kind() == nusb::ErrorKind::PermissionDenied
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_usb_open_error_is_never_treated_as_a_permission_denial() {
        assert!(!CliError::MissingProfile.is_permission_denied_usb_open());
    }

    #[test]
    fn a_permission_denied_write_after_a_successful_open_is_not_the_open_family_hint() {
        // `WriteUsb`'s source is a plain `std::io::Error`, so — unlike
        // `OpenUsbDevice`/`ClaimUsbInterface`/`OpenUsbOutEndpoint`, whose
        // `nusb::Error` source has no public constructor and so cannot be
        // built in a test at all — a `PermissionDenied`-kind fixture is
        // trivial to construct here. Confirms the predicate is scoped to
        // the open family on purpose, not merely because nothing else was
        // tested: a permission error surfacing after the device was already
        // opened successfully is a different condition than the one
        // `grant-usb-permissions` fixes, and must not trigger the hint.
        let error = CliError::WriteUsb {
            endpoint: 0x01,
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied"),
        };

        assert!(!error.is_permission_denied_usb_open());
    }
}
