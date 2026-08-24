//! The `printers discover` terminal adapter.

use std::future::{Future, pending, poll_fn};
use std::io::{self, ErrorKind, Write};
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use tokio::sync::{Notify, oneshot};

use super::super::cli::output::{
    NetworkListing, UsbListing, format_network_endpoint, usb_printer_label_parts,
    write_network_listing, write_usb_listing,
};
use super::super::cli::scan_announcement;
use super::super::cli::{DiscoverPrintersArgs, InventoryTransport};
use super::super::inventory::{NusbInventory, UsbEnumerationFailure, UsbFailureStage};
use super::{
    DiscoveryEvent, DiscoveryScope, NetworkDiscovery, NetworkScan, RegistrationAvailability,
    UsbDiscovery, execute, prepare,
};
use crate::discovery::SkippedInterface;
use crate::error::CliError;

/// The exit status of a sweep stopped with Ctrl+C: 128 + SIGINT, the shell's
/// convention for "died from an interrupt". Catching the signal to clear the
/// progress bar and print the closing hint must not cost a script the ability
/// to tell a stopped scan from a finished one, so the status still says the
/// run was interrupted.
const INTERRUPTED_EXIT_CODE: i32 = 130;

impl TryFrom<DiscoverPrintersArgs> for DiscoveryScope {
    type Error = CliError;

    fn try_from(arguments: DiscoverPrintersArgs) -> Result<Self, Self::Error> {
        if arguments.transport == Some(InventoryTransport::Usb) {
            if !arguments.subnet.is_empty()
                || arguments.port.is_some()
                || arguments.timeout.is_some()
            {
                return Err(CliError::NetworkScanOptionForUsbDiscovery);
            }
            return Ok(Self::Usb);
        }
        let scan = NetworkScan::new(
            arguments.port.unwrap_or(9100),
            arguments.subnet,
            Duration::from_millis(arguments.timeout.unwrap_or(1000)),
        )?;
        Ok(match arguments.transport {
            Some(InventoryTransport::Network) => Self::Network(scan),
            None => Self::All(scan),
            Some(InventoryTransport::Usb) => unreachable!("USB was handled above"),
        })
    }
}

/// The terminal's line for one skipped adapter: the shared reason, then the
/// flag that scans it anyway. The remedy is composed here rather than carried
/// by `SkippedInterface` because it is the terminal's alone — the workbench
/// answers the same omission by pointing at its custom-network field.
///
/// Shared with `printers add --discover`, which runs the same scan and so
/// must report the same omissions in the same words.
pub(in crate::features::printers) fn skipped_line(adapter: &SkippedInterface) -> String {
    match adapter.cli_hint() {
        Some(hint) => format!("Skipped {}, {hint}", adapter.describe()),
        None => format!("Skipped {}", adapter.describe()),
    }
}

pub(crate) async fn run_discover(
    arguments: DiscoverPrintersArgs,
    config: Option<PathBuf>,
) -> Result<(), CliError> {
    let scope = DiscoveryScope::try_from(arguments)?;
    let port = scope.network_scan().map_or(9100, NetworkScan::port);
    let prepared = prepare(config, scope)?;
    let bar = ProgressBar::with_draw_target(Some(0), ProgressDrawTarget::stderr());
    bar.set_style(
        ProgressStyle::with_template("{msg} [{bar:40}] {pos}/{len}")
            .expect("the progress bar template is a compile-time constant")
            .progress_chars("=> "),
    );
    bar.set_message("Scanning for network printers");
    let mut listing = LiveListing::new(&bar, io::stdout(), io::stderr());
    let output_closed = listing.output_closed_signal();
    let mut length_set = false;
    let mut inventory = NusbInventory;
    // Armed before the sweep, so no result can be printed before Ctrl+C is
    // caught. See `watch_for_interrupt`.
    let interrupted = watch_for_interrupt().await;
    // Ctrl+C stops the sweep only at a point where the command waits. The USB
    // scan does not wait, because it reads the devices in one step. A signal
    // that comes during that step waits until the step ends. With
    // `--transport usb` the command does not wait again after that step, so it
    // does not see the signal and it ends with a success status. A USB scan
    // takes a short time, so the delay is small.
    let finished = tokio::select! {
        result = execute(
            prepared,
            |event| match event {
                DiscoveryEvent::Prepared {
                    config_path,
                    scope,
                    scan_targets,
                    skipped,
                } => bar.suspend(|| {
                    // Suspended like every other write: the bar is already
                    // drawing on stderr by now, and an unsuspended line lands
                    // on top of it.
                    eprintln!("Reading configuration from {}", config_path.display());
                    if let Some(scan) = scope.network_scan() {
                        // Printed whenever an adapter was skipped, even if nothing is
                        // left to scan: a combined USB+network discovery still has USB
                        // work to do, and the omission must be reported either way.
                        for adapter in skipped {
                            eprintln!("{}", skipped_line(adapter));
                        }
                        if !scan_targets.is_empty() {
                            eprintln!("{}", scan_announcement(scan_targets, scan.port()));
                            if scan.uses_automatic_subnets() {
                                eprintln!("Tip: pass --subnet <CIDR> to scan a different network.");
                            }
                        }
                    }
                }),
                DiscoveryEvent::UsbPrinter(printer) => listing.usb_printer(printer),
                DiscoveryEvent::UsbFailure(failure) => listing.usb_failure(failure),
                DiscoveryEvent::NetworkPrinter(printer) => listing.network_printer(printer),
                DiscoveryEvent::NetworkScanProgress { completed, total } => {
                    if !length_set {
                        bar.set_length(total);
                        length_set = true;
                    }
                    bar.set_position(completed);
                }
            },
            &mut inventory,
        ) => Some(result),
        // Ctrl+C stops the sweep, not the process: everything already streamed
        // stays on stdout and the closing hint below still names the command
        // that registers it. Dropping the discovery future is exactly how the
        // web app cancels a scan when its response is dropped, so a terminal
        // and a browser abandon a sweep through one mechanism rather than two.
        _ = interrupted => None,
        // A reader that closes stdout asks for no more output. Stop the sweep
        // there instead of probing every remaining address into a pipe that
        // nobody reads.
        () = output_closed.notified() => None,
    };
    bar.finish_and_clear();
    // A closed stdout is the reader's decision, not a fault in the scan. The
    // command therefore ends without a message, without the closing hint, and
    // with a success status, the same way a reader expects `| head` to end.
    if listing.output_closed() {
        return Ok(());
    }
    let Some(result) = finished else {
        // Registration availability is tallied from what actually reached
        // stdout, because the `Response` that normally carries it was never
        // built. The hint therefore never names a printer the user never saw.
        print_registration_hint(listing.registration(), port);
        // Exit rather than return: a caught SIGINT must still look to the
        // shell like the interrupt it sent. Nothing here waits on anything, so
        // a second Ctrl+C has nothing to be trapped behind.
        std::process::exit(INTERRUPTED_EXIT_CODE);
    };
    // A stdout that could not be written outranks a discovery error: the
    // command has nowhere to report anything else.
    listing.take_error()?;
    let response = result?;
    listing.write_empty_notice()?;
    print_registration_hint(response.registration, port);
    Ok(())
}

/// Start to watch for Ctrl+C, and return only after the signal handler is
/// installed. The returned receiver resolves when the user asks for the sweep
/// to stop.
///
/// Tokio installs the handler at the first poll of its `ctrl_c` future. The
/// sweep can print its first results before that poll happens, because
/// `select!` polls its branches in an unspecified order and the sweep spawns
/// and collects many probes in one poll. A signal that comes in that gap gets
/// the default action, which ends the process and loses every result. The
/// task below therefore polls the future once and reports back, and the sweep
/// starts only after that.
///
/// A handler that cannot be installed must not cost the user their scan, so
/// the receiver then never resolves and the run finishes under the default
/// signal disposition.
async fn watch_for_interrupt() -> oneshot::Receiver<()> {
    let (installed, ready) = oneshot::channel();
    let (interrupted, asked_to_stop) = oneshot::channel();
    tokio::spawn(async move {
        let mut signal = pin!(tokio::signal::ctrl_c());
        let mut installed = Some(installed);
        let outcome = poll_fn(|context| {
            let polled = signal.as_mut().poll(context);
            if let Some(installed) = installed.take() {
                let _ = installed.send(());
            }
            polled
        })
        .await;
        if outcome.is_err() {
            pending::<()>().await;
        }
        let _ = interrupted.send(());
    });
    let _ = ready.await;
    asked_to_stop
}

fn print_registration_hint(registration: RegistrationAvailability, port: u16) {
    if let Some(hint) = combined_registration_hint(registration.usb, registration.network, port) {
        eprintln!("{hint}");
    }
}

/// Renders each printer the moment discovery announces it, so a sweep stopped
/// with Ctrl+C keeps everything it already found instead of discarding it.
///
/// The price is that `[N]` numbers follow arrival order rather than the stable
/// order of the final `Response`: USB printers first, then network hosts as
/// they answer. Nothing can be numbered against a list that does not exist
/// yet. The blocks themselves are the ones `printers list` writes, so the two
/// commands still cannot drift apart.
struct LiveListing<'a, O: Write, W: Write> {
    bar: &'a ProgressBar,
    output: O,
    warnings: W,
    printed: usize,
    registration: RegistrationAvailability,
    grant_hint_written: bool,
    /// The first failure to write a result. Kept rather than returned because
    /// the discovery observer this drives cannot report an error.
    error: Option<CliError>,
    /// Set when a write to the output finds a closed reader.
    output_closed: bool,
    /// Tells the sweep that the output closed. The observer runs inside the
    /// sweep, so it cannot stop the sweep itself. It sends this signal, and
    /// the caller waits for it beside the sweep.
    output_closed_signal: Arc<Notify>,
}

impl<'a, O: Write, W: Write> LiveListing<'a, O, W> {
    fn new(bar: &'a ProgressBar, output: O, warnings: W) -> Self {
        Self {
            bar,
            output,
            warnings,
            printed: 0,
            registration: RegistrationAvailability::default(),
            grant_hint_written: false,
            error: None,
            output_closed: false,
            output_closed_signal: Arc::new(Notify::new()),
        }
    }

    /// A handle on the "the output closed" signal, for the caller to wait on
    /// beside the sweep.
    fn output_closed_signal(&self) -> Arc<Notify> {
        Arc::clone(&self.output_closed_signal)
    }

    fn output_closed(&self) -> bool {
        self.output_closed
    }

    fn usb_printer(&mut self, discovered: &UsbDiscovery) {
        self.registration.usb |= discovered.configured_name.is_none();
        let number = self.printed + 1;
        self.write_result(|output| write_usb_discovery(output, number, discovered));
    }

    fn network_printer(&mut self, discovered: &NetworkDiscovery) {
        self.registration.network |= discovered.configured_names.is_empty();
        let number = self.printed + 1;
        self.write_result(|output| write_network_discovery(output, number, discovered));
    }

    /// A tolerated USB enumeration failure, on the warning stream. A write
    /// error here is dropped rather than kept: a diagnostic that cannot be
    /// printed must not become the command's own failure.
    fn usb_failure(&mut self, failure: &UsbEnumerationFailure) {
        let with_grant_hint = self.claim_grant_hint(failure);
        let Self { bar, warnings, .. } = self;
        let _ = bar.suspend(|| -> Result<(), CliError> {
            write_usb_failure(warnings, failure)?;
            if with_grant_hint {
                writeln!(
                    warnings,
                    "Fix USB permissions with: sudo escpost printers grant-usb-permissions"
                )
                .map_err(CliError::WriteHumanOutput)?;
            }
            warnings.flush().map_err(CliError::WriteHumanOutput)
        });
    }

    /// The closing line for a sweep that finished having found nothing.
    /// Suppressed once anything streamed, since the results are the report.
    fn write_empty_notice(&mut self) -> Result<(), CliError> {
        if self.printed > 0 || self.output_closed {
            return Ok(());
        }
        if let Err(error) =
            writeln!(self.output, "No printers discovered.").map_err(CliError::WriteHumanOutput)
        {
            self.record_write_failure(error);
        }
        self.take_error()
    }

    fn registration(&self) -> RegistrationAvailability {
        self.registration
    }

    fn take_error(&mut self) -> Result<(), CliError> {
        self.error.take().map_or(Ok(()), Err)
    }

    /// One result on stdout, drawn through the bar's `suspend` so it cannot
    /// land inside the bar's own line on stderr, and flushed immediately. A
    /// sweep that is stopped never closes its output, so a result that stays
    /// in a buffer is lost. The flush keeps every printed result, whatever
    /// writer is behind the output.
    fn write_result(&mut self, render: impl FnOnce(&mut O) -> Result<(), CliError>) {
        if self.error.is_some() || self.output_closed {
            return;
        }
        let Self { bar, output, .. } = self;
        let result = bar.suspend(|| {
            render(output)?;
            output.flush().map_err(CliError::WriteHumanOutput)
        });
        match result {
            Ok(()) => self.printed += 1,
            Err(error) => self.record_write_failure(error),
        }
    }

    /// Sort a failed write into the two answers it can get. A broken pipe
    /// means the reader closed the output, so the sweep stops and the command
    /// ends quietly. Every other failure is kept and reported.
    fn record_write_failure(&mut self, error: CliError) {
        if matches!(&error, CliError::WriteHumanOutput(cause) if cause.kind() == ErrorKind::BrokenPipe)
        {
            self.output_closed = true;
            self.output_closed_signal.notify_one();
            return;
        }
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    /// Whether this failure carries the "fix USB permissions" line. Only the
    /// first permission error does: the remedy is one command however many
    /// devices it unlocks. Never off Linux, where the udev rule it names has
    /// no equivalent. The line is written beside the warning that motivates it
    /// instead of after the listing, so an interrupted sweep keeps it too.
    fn claim_grant_hint(&mut self, failure: &UsbEnumerationFailure) -> bool {
        if !cfg!(target_os = "linux") || !failure.permission_denied || self.grant_hint_written {
            return false;
        }
        self.grant_hint_written = true;
        true
    }
}

fn write_usb_discovery(
    output: &mut impl Write,
    number: usize,
    discovered: &UsbDiscovery,
) -> Result<(), CliError> {
    let product = usb_printer_label_parts(discovered.printer.product.as_deref(), None);
    let listing = match &discovered.configured_name {
        Some(name) => UsbListing {
            heading: name,
            status: "configured",
            model: Some(product.as_str()),
            profile: Some(discovered.configured_profile.as_deref()),
            printer: &discovered.printer,
        },
        None => UsbListing {
            heading: &product,
            status: "new",
            model: None,
            profile: None,
            printer: &discovered.printer,
        },
    };
    write_usb_listing(output, number, &listing)
}

fn write_network_discovery(
    output: &mut impl Write,
    number: usize,
    discovered: &NetworkDiscovery,
) -> Result<(), CliError> {
    let endpoint = format_network_endpoint(&discovered.host, discovered.port);
    let also_configured = discovered
        .configured_names
        .iter()
        .skip(1)
        .map(String::as_str)
        .collect::<Vec<_>>();
    let listing = if let Some(first) = discovered.configured_names.first() {
        NetworkListing {
            heading: first,
            status: "configured",
            profile: Some(discovered.configured_profile.as_deref()),
            host: &discovered.host,
            port: discovered.port,
            interface: discovered.interface.as_deref(),
            also_configured: &also_configured,
        }
    } else {
        NetworkListing {
            heading: &endpoint,
            status: "new",
            profile: None,
            host: &discovered.host,
            port: discovered.port,
            interface: discovered.interface.as_deref(),
            also_configured: &[],
        }
    };
    write_network_listing(output, number, &listing)
}

fn write_usb_failure(
    output: &mut impl Write,
    failure: &UsbEnumerationFailure,
) -> Result<(), CliError> {
    let action = match failure.stage {
        UsbFailureStage::OpenDevice => "could not open",
        UsbFailureStage::InspectConfiguration => "could not inspect the active configuration of",
    };
    writeln!(
        output,
        "Warning: {action} USB device {:04x}:{:04x}: {}",
        failure.vendor_id, failure.product_id, failure.reason
    )
    .map_err(CliError::WriteHumanOutput)
}

/// Choose the single registration hint from the typed response's availability
/// facts. `None` means neither transport found an unconfigured printer.
fn combined_registration_hint(new_usb: bool, new_network: bool, port: u16) -> Option<String> {
    match (new_usb, new_network) {
        (false, false) => None,
        (true, false) => Some(
            "Register a new USB printer with: escpost printers add <NAME> --transport usb"
                .to_owned(),
        ),
        (false, true) => {
            let port_suffix = if port == 9100 {
                String::new()
            } else {
                format!(" --port {port}")
            };
            Some(format!(
                "Register a new network printer with: escpost printers add <NAME> --transport network --discover{port_suffix}"
            ))
        }
        (true, true) => Some("Register a new printer with: escpost printers add <NAME>".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;
    use crate::application::ApplicationError;
    use crate::discovery::ScanTarget;
    use crate::discovery::{SkipReason, Subnet};
    use crate::features::printers::discover::{
        DiscoveryScope, NetworkDiscovery, NetworkScan, RegistrationAvailability, UsbDiscovery,
    };
    use crate::features::printers::inventory::UsbPrinter;

    /// Moving the remedy out of `SkippedInterface::describe` must leave the
    /// terminal saying exactly what it said before, flag included.
    #[test]
    fn the_skipped_line_still_names_the_flag_that_scans_the_adapter() {
        let too_large = SkippedInterface {
            name: "enp5s0".to_owned(),
            subnet: Some(Subnet::parse("10.0.0.0/16").expect("valid subnet")),
            reason: SkipReason::TooLarge,
        };
        let unusable = SkippedInterface {
            name: "weird0".to_owned(),
            subnet: None,
            reason: SkipReason::UnusableNetmask,
        };

        assert_eq!(
            skipped_line(&too_large),
            "Skipped enp5s0 (10.0.0.0/16): larger than /24, scan it with --subnet 10.0.0.0/16"
        );
        assert_eq!(
            skipped_line(&unusable),
            "Skipped weird0: its netmask does not name a scannable subnet"
        );
    }

    #[test]
    fn cli_arguments_convert_to_each_valid_discovery_scope() {
        let subnet = Subnet::parse("10.42.0.71/24").expect("valid subnet");
        let cases = [
            (
                DiscoverPrintersArgs {
                    transport: Some(InventoryTransport::Usb),
                    port: None,
                    subnet: Vec::new(),
                    timeout: None,
                },
                DiscoveryScope::Usb,
            ),
            (
                DiscoverPrintersArgs {
                    transport: Some(InventoryTransport::Network),
                    port: Some(9200),
                    subnet: vec![subnet],
                    timeout: Some(75),
                },
                DiscoveryScope::Network(
                    NetworkScan::new(9200, vec![subnet], Duration::from_millis(75))
                        .expect("the expected network scan should be valid"),
                ),
            ),
            (
                DiscoverPrintersArgs {
                    transport: None,
                    port: None,
                    subnet: Vec::new(),
                    timeout: None,
                },
                DiscoveryScope::All(
                    NetworkScan::new(9100, Vec::new(), Duration::from_millis(1000))
                        .expect("the expected combined scan should be valid"),
                ),
            ),
        ];

        for (arguments, expected) in cases {
            assert_eq!(DiscoveryScope::try_from(arguments).unwrap(), expected);
        }
    }

    #[test]
    fn every_network_option_combination_is_rejected_for_usb_discovery() {
        let subnet = Subnet::parse("127.0.0.1/32").expect("valid subnet");
        for option_mask in 1u8..=7 {
            let arguments = DiscoverPrintersArgs {
                transport: Some(InventoryTransport::Usb),
                port: (option_mask & 0b001 != 0).then_some(9100),
                subnet: if option_mask & 0b010 != 0 {
                    vec![subnet]
                } else {
                    Vec::new()
                },
                timeout: (option_mask & 0b100 != 0).then_some(1000),
            };

            assert!(matches!(
                DiscoveryScope::try_from(arguments),
                Err(CliError::NetworkScanOptionForUsbDiscovery)
            ));
        }
    }

    #[test]
    fn zero_port_is_rejected_while_converting_network_and_all_scopes() {
        for transport in [Some(InventoryTransport::Network), None] {
            let error = DiscoveryScope::try_from(DiscoverPrintersArgs {
                transport,
                port: Some(0),
                subnet: Vec::new(),
                timeout: None,
            })
            .expect_err("zero is not a valid network discovery port");

            assert!(matches!(
                error,
                CliError::Application(ApplicationError::InvalidPrinterPort)
            ));
        }
    }

    /// Streamed results carry the same blocks the command printed when it
    /// rendered the finished `Response`, numbered continuously in the order
    /// discovery announced them: USB printers first, then network hosts.
    #[test]
    fn streaming_numbers_results_continuously_in_arrival_order() {
        let bar = ProgressBar::hidden();
        let mut listing = LiveListing::new(&bar, Vec::new(), Vec::new());

        listing.usb_printer(&UsbDiscovery {
            configured_name: None,
            configured_profile: None,
            printer: usb_printer("003", 60, Some("B120300001"), Some("YICHIP3121")),
        });
        listing.usb_printer(&UsbDiscovery {
            configured_name: Some("counter".to_owned()),
            configured_profile: None,
            printer: usb_printer("004", 61, Some("B120300002"), None),
        });
        listing.usb_failure(&UsbEnumerationFailure {
            stage: UsbFailureStage::InspectConfiguration,
            vendor_id: 0x0416,
            product_id: 0x5012,
            reason: "device is not configured".to_owned(),
            permission_denied: false,
        });
        listing.network_printer(&NetworkDiscovery {
            configured_names: Vec::new(),
            configured_profile: None,
            host: "10.42.0.5".to_owned(),
            port: 9100,
            interface: None,
        });
        listing.network_printer(&NetworkDiscovery {
            configured_names: vec!["kitchen".to_owned(), "kitchen-spare".to_owned()],
            configured_profile: Some("TM-T88V".to_owned()),
            host: "2001:db8::5".to_owned(),
            port: 9100,
            interface: Some("enx0".to_owned()),
        });
        // Results were streamed, so nothing claims an empty sweep.
        listing
            .write_empty_notice()
            .expect("the listing should be writable");

        assert_eq!(
            String::from_utf8(listing.output).expect("the listing should be UTF-8"),
            "\
[1] USB Portable Printer
    status: new
    manufacturer: YICHIP3121
    transport: usb
    usb: 0416:5011; bus 003 address 60; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300001
[2] counter
    status: configured
    model: USB Portable Printer
    profile: unassigned
    transport: usb
    usb: 0416:5011; bus 004 address 61; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300002
[3] 10.42.0.5:9100
    status: new
    transport: network
    network: 10.42.0.5:9100
[4] kitchen
    status: configured
    profile: TM-T88V
    transport: network
    network: [2001:db8::5]:9100
    interface: enx0
    also configured as: kitchen-spare
"
        );
        assert_eq!(
            String::from_utf8(listing.warnings).expect("the warnings should be UTF-8"),
            "Warning: could not inspect the active configuration of USB device 0416:5012: device is not configured\n"
        );
    }

    /// An interrupted sweep has no `Response` to read availability from, so
    /// the closing hint is chosen from what was streamed. Only a `status: new`
    /// result may enable a transport's hint.
    #[test]
    fn streaming_tallies_registration_from_unconfigured_results_only() {
        let bar = ProgressBar::hidden();
        let mut listing = LiveListing::new(&bar, Vec::new(), Vec::new());

        listing.usb_printer(&UsbDiscovery {
            configured_name: Some("counter".to_owned()),
            configured_profile: None,
            printer: usb_printer("004", 61, Some("B120300002"), None),
        });
        listing.network_printer(&NetworkDiscovery {
            configured_names: vec!["kitchen".to_owned()],
            configured_profile: None,
            host: "10.42.0.5".to_owned(),
            port: 9100,
            interface: None,
        });
        assert_eq!(
            listing.registration(),
            RegistrationAvailability {
                usb: false,
                network: false
            }
        );

        listing.network_printer(&NetworkDiscovery {
            configured_names: Vec::new(),
            configured_profile: None,
            host: "10.42.0.6".to_owned(),
            port: 9100,
            interface: None,
        });

        assert_eq!(
            listing.registration(),
            RegistrationAvailability {
                usb: false,
                network: true
            }
        );
    }

    #[test]
    fn a_sweep_that_streamed_nothing_reports_an_empty_snapshot() {
        let bar = ProgressBar::hidden();
        let mut listing = LiveListing::new(&bar, Vec::new(), Vec::new());

        listing
            .write_empty_notice()
            .expect("the empty listing should be writable");

        assert_eq!(listing.output, b"No printers discovered.\n");
        assert!(listing.warnings.is_empty());
    }

    /// A writer that records how much output each flush pushed out.
    #[derive(Default)]
    struct FlushRecorder {
        written: Vec<u8>,
        /// The size of `written` at each flush.
        flushed_at: Vec<usize>,
    }

    impl Write for FlushRecorder {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushed_at.push(self.written.len());
            Ok(())
        }
    }

    /// Each result must leave the program as soon as it is complete, because
    /// a sweep that is stopped never gets a chance to empty a buffer. Only
    /// the flush proves that: a buffered output holds the same bytes but
    /// loses every one of them when the sweep stops.
    #[test]
    fn each_streamed_result_is_flushed_as_soon_as_it_is_written() {
        let bar = ProgressBar::hidden();
        let mut listing = LiveListing::new(&bar, FlushRecorder::default(), Vec::new());

        listing.usb_printer(&UsbDiscovery {
            configured_name: None,
            configured_profile: None,
            printer: usb_printer("003", 60, Some("B120300001"), Some("YICHIP3121")),
        });
        let after_first = listing.output.written.len();
        assert!(after_first > 0, "the first result should have been written");
        assert_eq!(
            listing.output.flushed_at,
            vec![after_first],
            "the first result should be flushed as a whole block, once"
        );

        listing.network_printer(&NetworkDiscovery {
            configured_names: Vec::new(),
            configured_profile: None,
            host: "10.42.0.5".to_owned(),
            port: 9100,
            interface: None,
        });
        let after_second = listing.output.written.len();
        assert!(
            after_second > after_first,
            "the second result should have been written"
        );
        assert_eq!(
            listing.output.flushed_at,
            vec![after_first, after_second],
            "the second result should be flushed as a whole block too"
        );
    }

    /// A writer whose reader has gone away.
    struct ClosedOutput;

    impl Write for ClosedOutput {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(ErrorKind::BrokenPipe))
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::from(ErrorKind::BrokenPipe))
        }
    }

    /// `printers discover | head -n 1` closes the output while the sweep
    /// runs. That is the reader's decision, so the listing keeps no error and
    /// tells the sweep to stop instead of probing every address that is left.
    #[tokio::test]
    async fn a_closed_output_stops_the_sweep_without_an_error() {
        let bar = ProgressBar::hidden();
        let mut listing = LiveListing::new(&bar, ClosedOutput, Vec::new());
        let closed = listing.output_closed_signal();

        listing.network_printer(&NetworkDiscovery {
            configured_names: Vec::new(),
            configured_profile: None,
            host: "10.42.0.5".to_owned(),
            port: 9100,
            interface: None,
        });

        assert!(listing.output_closed(), "the closed output should be seen");
        listing
            .take_error()
            .expect("a closed output is not a command failure");
        listing
            .write_empty_notice()
            .expect("a closed output must not turn into an empty-sweep failure");
        tokio::time::timeout(Duration::from_secs(5), closed.notified())
            .await
            .expect("the sweep should be told to stop");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn streamed_permission_failures_carry_one_grant_hint() {
        let bar = ProgressBar::hidden();
        let mut listing = LiveListing::new(&bar, Vec::new(), Vec::new());

        for product_id in [0x5012, 0x5013] {
            listing.usb_failure(&UsbEnumerationFailure {
                stage: UsbFailureStage::OpenDevice,
                vendor_id: 0x0416,
                product_id,
                reason: "permission denied (errno 13)".to_owned(),
                permission_denied: true,
            });
        }

        // The hint follows the first permission error rather than the whole
        // listing, so a sweep stopped mid-scan still carries the remedy.
        assert_eq!(
            String::from_utf8(listing.warnings).expect("the warnings should be UTF-8"),
            "\
Warning: could not open USB device 0416:5012: permission denied (errno 13)
Fix USB permissions with: sudo escpost printers grant-usb-permissions
Warning: could not open USB device 0416:5013: permission denied (errno 13)
"
        );
    }

    #[test]
    fn registration_hints_cover_each_typed_availability_shape() {
        assert_eq!(combined_registration_hint(false, false, 9100), None);
        assert_eq!(
            combined_registration_hint(true, false, 9100),
            Some(
                "Register a new USB printer with: escpost printers add <NAME> --transport usb"
                    .to_owned()
            )
        );
        assert_eq!(
            combined_registration_hint(false, true, 9100),
            Some(
                "Register a new network printer with: escpost printers add <NAME> --transport network --discover"
                    .to_owned()
            )
        );
        assert_eq!(
            combined_registration_hint(false, true, 9200),
            Some(
                "Register a new network printer with: escpost printers add <NAME> --transport network --discover --port 9200"
                    .to_owned()
            )
        );
        assert_eq!(
            combined_registration_hint(true, true, 9200),
            Some("Register a new printer with: escpost printers add <NAME>".to_owned())
        );
    }

    #[test]
    fn scan_announcement_lists_mixed_targets_and_interfaces() {
        let targets = vec![
            ScanTarget {
                subnet: Subnet::parse("10.42.0.0/24").expect("valid subnet"),
                interface: Some("enx0".to_owned()),
                excluded: vec![Ipv4Addr::new(10, 42, 0, 9)],
            },
            ScanTarget {
                subnet: Subnet::parse("192.168.50.0/24").expect("valid subnet"),
                interface: None,
                excluded: Vec::new(),
            },
        ];

        assert_eq!(
            scan_announcement(&targets, 9100),
            "Scanning 2 networks on port 9100 (507 addresses):\n  - 10.42.0.0/24 (enx0)\n  - 192.168.50.0/24"
        );
    }

    #[test]
    fn scan_announcement_uses_the_singular_for_one_target() {
        let targets = vec![ScanTarget {
            subnet: Subnet::parse("10.42.0.0/24").expect("valid subnet"),
            interface: None,
            excluded: Vec::new(),
        }];

        assert_eq!(
            scan_announcement(&targets, 9200),
            "Scanning 1 network on port 9200 (254 addresses):\n  - 10.42.0.0/24"
        );
    }

    fn usb_printer(
        bus: &str,
        address: u8,
        serial_number: Option<&str>,
        manufacturer: Option<&str>,
    ) -> UsbPrinter {
        UsbPrinter {
            vendor_id: 0x0416,
            product_id: 0x5011,
            bus: bus.to_owned(),
            address,
            manufacturer: manufacturer.map(str::to_owned),
            product: Some("USB Portable Printer".to_owned()),
            serial_number: serial_number.map(str::to_owned),
            interface_number: 0,
            out_endpoints: vec![0x01],
            in_endpoints: vec![0x81],
        }
    }
}
