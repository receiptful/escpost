use std::fmt;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

use thiserror::Error;

use crate::application::ApplicationError;

#[derive(Debug, Error)]
pub(crate) enum CliError {
    #[error("{}", format_application_error(.0))]
    Application(#[source] ApplicationError),

    #[error("printer profile is required; pass --profile REFERENCE for generic rendering")]
    MissingProfile,

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

    #[error("could not write PNG output to stdout: {0}")]
    WriteStdout(#[source] std::io::Error),

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
    ServeWeb(#[source] std::io::Error),

    #[error("could not bind RAW printer to {address}: {source}")]
    BindRawPrinter {
        address: std::net::SocketAddr,
        source: std::io::Error,
    },

    #[error("no loopback RAW printer port from 9100 through 9109 is available")]
    NoAutomaticRawPort,

    #[error("RAW printer failed: {0}")]
    ServeRawPrinter(#[source] std::io::Error),

    #[error("idle timeout must be a positive number of seconds")]
    InvalidIdleTimeout,

    #[error("watch mode requires a filesystem source, not stdin")]
    WatchStdin,

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

    #[error("printer is required; pass --printer <NAME>")]
    MissingPrintPrinter,

    #[error("could not write command output: {0}")]
    WriteHumanOutput(#[source] std::io::Error),

    #[error("could not serialize JSON command output: {0}")]
    SerializeJsonOutput(#[source] serde_json::Error),

    #[error("printer name is required")]
    MissingPrinterName,

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

    #[error("could not read printer information: {0}")]
    PrinterPrompt(String),

    #[cfg(target_os = "linux")]
    #[error("granting USB printer access requires root\n\n{guidance}")]
    GrantUsbPermissionsNeedsRoot { guidance: String },

    #[cfg(target_os = "linux")]
    #[error("could not read the confirmation: {0}")]
    ConfirmationPrompt(String),

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

impl From<ApplicationError> for CliError {
    fn from(error: ApplicationError) -> Self {
        Self::Application(error)
    }
}

fn format_application_error(error: &ApplicationError) -> String {
    let mut message = error.to_string();
    match error {
        ApplicationError::UnknownConfiguredPrinter(_) => {
            message.push_str("; use `escpost printers list` to see available names");
        }
        ApplicationError::NoDiscoverableSubnets => {
            message.push_str("; pass --subnet <CIDR>");
        }
        _ => {}
    }
    message
}

struct CliErrorMessage<'a>(&'a CliError);

impl fmt::Display for CliErrorMessage<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)?;
        #[cfg(target_os = "linux")]
        if self.0.is_permission_denied_usb_open() {
            formatter.write_str(
                "\nFix USB permissions with: sudo escpost printers grant-usb-permissions",
            )?;
        }
        Ok(())
    }
}

impl CliError {
    pub(crate) fn display_message(&self) -> impl fmt::Display + '_ {
        CliErrorMessage(self)
    }

    /// Delegate USB-open permission classification to the factual application
    /// failure. Adapter failures cannot represent this condition.
    pub(crate) fn is_permission_denied_usb_open(&self) -> bool {
        match self {
            Self::Application(error) => error.is_permission_denied_usb_open(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn factual_application_error_has_no_cli_guidance() {
        let error = ApplicationError::UnknownConfiguredPrinter("counter".to_owned());

        assert_eq!(error.to_string(), "printer \"counter\" is not configured");
    }

    #[test]
    fn configured_printer_failure_keeps_cli_guidance() {
        let error = CliError::from(ApplicationError::UnknownConfiguredPrinter(
            "counter".to_owned(),
        ));

        assert_eq!(
            error.display_message().to_string(),
            "printer \"counter\" is not configured; use `escpost printers list` to see available names"
        );
    }

    #[test]
    fn automatic_discovery_failure_keeps_cli_guidance() {
        let error = CliError::from(ApplicationError::NoDiscoverableSubnets);

        assert_eq!(
            error.display_message().to_string(),
            "no directly connected IPv4 network is small enough to scan automatically (at most /24); pass --subnet <CIDR>"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn usb_open_permission_failure_adds_cli_recovery_guidance() {
        let error = CliError::from(ApplicationError::OpenUsbDevice {
            vendor_id: 0x0416,
            product_id: 0x5011,
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied"),
        });

        assert_eq!(
            error.display_message().to_string(),
            "could not open USB device 0x0416:0x5011: permission denied\nFix USB permissions with: sudo escpost printers grant-usb-permissions"
        );
    }

    #[test]
    fn a_non_usb_open_error_is_never_treated_as_a_permission_denial() {
        assert!(!CliError::MissingProfile.is_permission_denied_usb_open());
    }

    #[test]
    fn a_permission_denied_write_after_a_successful_open_is_not_the_open_family_hint() {
        let error = CliError::from(ApplicationError::WriteUsb {
            endpoint: 0x01,
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied"),
        });

        assert!(!error.is_permission_denied_usb_open());
    }

    #[test]
    fn wrapped_cli_io_error_remains_in_the_source_chain() {
        let error = CliError::WriteStdout(std::io::Error::other("stdout disconnected"));

        assert_eq!(
            error.source().map(ToString::to_string).as_deref(),
            Some("stdout disconnected")
        );
    }

    #[test]
    fn wrapped_cli_serialization_error_remains_in_the_source_chain() {
        let source = serde_json::from_str::<serde_json::Value>("{")
            .expect_err("the incomplete object should fail to parse");
        let error = CliError::SerializeJsonOutput(source);

        assert!(error.source().is_some());
    }
}
