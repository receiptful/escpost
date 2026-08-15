use std::fmt;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

use crate::cli::PrintArgs;
use crate::configuration::{self, ConfiguredPrinter};
use crate::error::CliError;
use crate::features::printers::cli as printers;
use crate::source;
use inquire::Select;
use nusb::MaybeFuture;
use nusb::transfer::{Bulk, Out};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

const USB_WRITE_BUFFER_BYTES: usize = 16 * 1024;
const USB_TRANSFER_TIMEOUT: Duration = Duration::from_secs(10);
const NETWORK_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const NETWORK_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
struct UsbTarget {
    vendor_id: u16,
    product_id: u16,
    serial_number: Option<String>,
    interface: u8,
    out_endpoint: u8,
}

impl fmt::Display for UsbTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04x}:{:04x}, interface {}, OUT {:#04x}",
            self.vendor_id, self.product_id, self.interface, self.out_endpoint
        )?;
        if let Some(serial_number) = &self.serial_number {
            write!(formatter, ", serial {serial_number}")?;
        }
        Ok(())
    }
}

struct PrintReport {
    printer: String,
    target: PrintTarget,
    bytes_sent: usize,
}

enum PrintTarget {
    Usb(UsbTarget),
    Network(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PrinterChoice {
    Printer {
        name: String,
        transport: &'static str,
        profile: Option<String>,
    },
    Add,
}

trait UsbTransport {
    fn send(&mut self, target: &UsbTarget, data: &[u8]) -> Result<(), CliError>;
}

trait PrinterSelector {
    fn select(&mut self, choices: Vec<PrinterChoice>) -> Result<PrinterChoice, CliError>;
}

trait PrinterAdder {
    fn add(&mut self, config_path: Option<&Path>) -> Result<String, CliError>;
}

pub(crate) async fn run(arguments: PrintArgs, non_interactive: bool) -> Result<(), CliError> {
    let can_prompt = !non_interactive && io::stdin().is_terminal() && io::stderr().is_terminal();
    let mut transport = NusbTransport;
    let report = execute(
        arguments,
        can_prompt,
        &mut InquirePrinterSelector,
        &mut InquirePrinterAdder,
        &mut transport,
    )
    .await?;
    eprintln!("Printer: {}", report.printer);
    match &report.target {
        PrintTarget::Usb(target) => {
            eprintln!("Transport: usb");
            eprintln!("USB target: {target}");
        }
        PrintTarget::Network(target) => {
            eprintln!("Transport: network");
            eprintln!("Network target: {target}");
        }
    }
    eprintln!("Bytes sent: {}", report.bytes_sent);
    Ok(())
}

async fn execute(
    arguments: PrintArgs,
    can_prompt: bool,
    selector: &mut impl PrinterSelector,
    adder: &mut impl PrinterAdder,
    transport: &mut impl UsbTransport,
) -> Result<PrintReport, CliError> {
    let printer_name = resolve_printer_name(
        arguments.printer,
        arguments.config.as_deref(),
        can_prompt,
        selector,
        adder,
    )?;
    let configuration = configuration::load(arguments.config.as_deref())?;
    let printer = configuration
        .printer(&printer_name)
        .ok_or_else(|| CliError::UnknownConfiguredPrinter(printer_name.clone()))?;
    let input = source::load(&arguments.source, arguments.format)?;
    let target = match printer {
        ConfiguredPrinter::Usb(printer) => {
            let target = UsbTarget {
                vendor_id: printer.vendor_id,
                product_id: printer.product_id,
                serial_number: printer.serial_number.clone(),
                interface: printer.interface_number,
                out_endpoint: printer.out_endpoint,
            };
            if !(0x01..=0x0f).contains(&target.out_endpoint) {
                return Err(CliError::InvalidUsbOutEndpoint(target.out_endpoint));
            }
            transport.send(&target, &input.bytes)?;
            PrintTarget::Usb(target)
        }
        ConfiguredPrinter::Network(printer) => {
            let target = format_network_endpoint(&printer.host, printer.port);
            send_network(&printer.host, printer.port, &target, &input.bytes).await?;
            PrintTarget::Network(target)
        }
    };
    Ok(PrintReport {
        printer: printer_name,
        target,
        bytes_sent: input.bytes.len(),
    })
}

fn resolve_printer_name(
    explicit_name: Option<String>,
    config_path: Option<&Path>,
    can_prompt: bool,
    selector: &mut impl PrinterSelector,
    adder: &mut impl PrinterAdder,
) -> Result<String, CliError> {
    if let Some(name) = explicit_name {
        return Ok(name);
    }
    if !can_prompt {
        return Err(CliError::MissingPrintPrinter);
    }

    // Selecting "Add a printer…" must also work on a fresh installation,
    // where the explicit configuration path does not exist until the add
    // workflow creates it.
    let configuration = configuration::load_for_update(config_path)?;
    let mut choices: Vec<_> = configuration
        .printers()
        .map(|printer| PrinterChoice::Printer {
            name: printer.name().to_owned(),
            transport: printer.transport(),
            profile: printer.profile().map(str::to_owned),
        })
        .collect();
    choices.sort_by(|left, right| {
        choice_name(left)
            .to_lowercase()
            .cmp(&choice_name(right).to_lowercase())
            .then_with(|| choice_name(left).cmp(choice_name(right)))
    });
    choices.push(PrinterChoice::Add);

    match selector.select(choices)? {
        PrinterChoice::Printer { name, .. } => Ok(name),
        PrinterChoice::Add => adder.add(config_path),
    }
}

fn choice_name(choice: &PrinterChoice) -> &str {
    match choice {
        PrinterChoice::Printer { name, .. } => name,
        PrinterChoice::Add => "",
    }
}

struct InquirePrinterSelector;

impl PrinterSelector for InquirePrinterSelector {
    fn select(&mut self, choices: Vec<PrinterChoice>) -> Result<PrinterChoice, CliError> {
        Select::new("Printer", choices)
            .prompt()
            .map_err(|error| CliError::PrinterPrompt(error.to_string()))
    }
}

struct InquirePrinterAdder;

impl PrinterAdder for InquirePrinterAdder {
    fn add(&mut self, config_path: Option<&Path>) -> Result<String, CliError> {
        printers::add_interactively(config_path)
    }
}

impl fmt::Display for PrinterChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Printer {
                name,
                transport,
                profile,
            } => write!(
                formatter,
                "{name} ({transport}, profile: {})",
                profile.as_deref().unwrap_or("unassigned")
            ),
            Self::Add => formatter.write_str("Add a printer…"),
        }
    }
}

async fn send_network(host: &str, port: u16, target: &str, data: &[u8]) -> Result<(), CliError> {
    let mut stream = timeout(NETWORK_CONNECT_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .map_err(|_| CliError::ConnectNetworkPrinterTimeout(target.to_owned()))?
        .map_err(|source| CliError::ConnectNetworkPrinter {
            target: target.to_owned(),
            source,
        })?;

    timeout(NETWORK_WRITE_TIMEOUT, async {
        stream.write_all(data).await?;
        stream.shutdown().await
    })
    .await
    .map_err(|_| CliError::WriteNetworkPrinterTimeout(target.to_owned()))?
    .map_err(|source| CliError::WriteNetworkPrinter {
        target: target.to_owned(),
        source,
    })
}

fn format_network_endpoint(host: &str, port: u16) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

struct NusbTransport;

impl UsbTransport for NusbTransport {
    fn send(&mut self, target: &UsbTarget, data: &[u8]) -> Result<(), CliError> {
        let matches: Vec<_> = nusb::list_devices()
            .wait()
            .map_err(CliError::EnumerateUsb)?
            .filter(|device| {
                device.vendor_id() == target.vendor_id
                    && device.product_id() == target.product_id
                    && target
                        .serial_number
                        .as_deref()
                        .is_none_or(|serial_number| device.serial_number() == Some(serial_number))
            })
            .collect();
        let device_info = require_unique_device(matches, target)?;
        let device = device_info
            .open()
            .wait()
            .map_err(|source| CliError::OpenUsbDevice {
                vendor_id: target.vendor_id,
                product_id: target.product_id,
                source,
            })?;
        // On Linux this temporarily detaches a kernel driver such as usblp.
        // nusb reattaches that driver when the claimed interface is dropped.
        let interface = device
            .detach_and_claim_interface(target.interface)
            .wait()
            .map_err(|source| CliError::ClaimUsbInterface {
                interface: target.interface,
                source,
            })?;
        let endpoint = interface
            .endpoint::<Bulk, Out>(target.out_endpoint)
            .map_err(|source| CliError::OpenUsbOutEndpoint {
                interface: target.interface,
                endpoint: target.out_endpoint,
                source,
            })?;
        let mut writer = endpoint
            .writer(USB_WRITE_BUFFER_BYTES)
            .with_write_timeout(USB_TRANSFER_TIMEOUT);

        // ESC/POS is already the wire format. Do not prepend initialization,
        // append paper motion, or otherwise alter the caller's bytes.
        writer
            .write_all(data)
            .map_err(|source| CliError::WriteUsb {
                endpoint: target.out_endpoint,
                source,
            })?;
        writer.flush().map_err(|source| CliError::FlushUsb {
            endpoint: target.out_endpoint,
            source,
        })
    }
}

fn require_unique_device<T>(mut matches: Vec<T>, target: &UsbTarget) -> Result<T, CliError> {
    match matches.len() {
        0 => Err(CliError::UsbDeviceNotFound {
            vendor_id: target.vendor_id,
            product_id: target.product_id,
        }),
        1 => Ok(matches.remove(0)),
        count => Err(CliError::AmbiguousUsbDevices {
            vendor_id: target.vendor_id,
            product_id: target.product_id,
            count,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        PrintTarget, PrinterAdder, PrinterChoice, PrinterSelector, UsbTarget, UsbTransport,
        execute, require_unique_device,
    };
    use crate::cli::{InputFormat, PrintArgs};
    use crate::error::CliError;

    #[tokio::test]
    async fn hexadecimal_source_bytes_reach_the_named_usb_boundary_unchanged() {
        let directory = temporary_directory("exact-hex");
        let source = directory.join("receipt.hex");
        let configuration = directory.join("printers.toml");
        fs::write(&source, "1b 40 00 ff 0a\n").expect("the source should be writable");
        fs::write(
            &configuration,
            "\
[counter]
transport = \"usb\"
profile = \"REFERENCE\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
serial_number = \"B120300001\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("the printer configuration should be writable");
        let arguments = PrintArgs {
            source,
            format: InputFormat::Auto,
            printer: Some("counter".to_owned()),
            config: Some(configuration),
        };
        let mut transport = RecordingTransport::default();

        let report = execute(
            arguments,
            false,
            &mut UnexpectedSelector,
            &mut UnexpectedAdder,
            &mut transport,
        )
        .await
        .expect("printing should succeed");

        assert_eq!(
            transport.request,
            Some((
                UsbTarget {
                    vendor_id: 0x0416,
                    product_id: 0x5011,
                    serial_number: Some("B120300001".to_owned()),
                    interface: 0,
                    out_endpoint: 0x01,
                },
                vec![0x1b, 0x40, 0x00, 0xff, 0x0a],
            ))
        );
        assert_eq!(report.printer, "counter");
        assert_eq!(
            match report.target {
                PrintTarget::Usb(target) => target,
                PrintTarget::Network(target) => {
                    panic!("expected a USB target, got network target {target}")
                }
            },
            UsbTarget {
                vendor_id: 0x0416,
                product_id: 0x5011,
                serial_number: Some("B120300001".to_owned()),
                interface: 0,
                out_endpoint: 0x01,
            }
        );
        assert_eq!(report.bytes_sent, 5);
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[tokio::test]
    async fn interactive_selection_prints_to_the_chosen_named_printer() {
        let directory = temporary_directory("interactive-existing");
        let source = directory.join("receipt.hex");
        let configuration = directory.join("printers.toml");
        fs::write(&source, "1b 40 0a\n").expect("the source should be writable");
        fs::write(
            &configuration,
            "\
[counter]
transport = \"usb\"
profile = \"REFERENCE\"
vendor_id = \"0x0416\"
product_id = \"0x5011\"
interface_number = 0
out_endpoint = \"0x01\"
",
        )
        .expect("the printer configuration should be writable");
        let arguments = PrintArgs {
            source,
            format: InputFormat::Auto,
            printer: None,
            config: Some(configuration),
        };
        let mut selector = FixedSelector;
        let mut transport = RecordingTransport::default();

        let report = execute(
            arguments,
            true,
            &mut selector,
            &mut UnexpectedAdder,
            &mut transport,
        )
        .await
        .expect("the selected printer should receive the job");

        assert_eq!(report.printer, "counter");
        assert_eq!(
            transport.request.expect("USB should receive the job").1,
            vec![0x1b, 0x40, 0x0a]
        );
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[tokio::test]
    async fn interactive_addition_prints_to_the_newly_configured_printer() {
        let directory = temporary_directory("interactive-add");
        let source = directory.join("receipt.bin");
        let configuration = directory.join("printers.toml");
        let expected = b"\x1b@New printer\n";
        fs::write(&source, expected).expect("the source should be writable");
        let listener =
            TcpListener::bind(("127.0.0.1", 0)).expect("the loopback printer should bind");
        let port = listener
            .local_addr()
            .expect("the listener should have an address")
            .port();
        let receiver = thread::spawn(move || {
            let (mut connection, _) = listener
                .accept()
                .expect("the new printer should receive a connection");
            let mut bytes = Vec::new();
            connection
                .read_to_end(&mut bytes)
                .expect("the print connection should close cleanly");
            bytes
        });
        let arguments = PrintArgs {
            source,
            format: InputFormat::Auto,
            printer: None,
            config: Some(configuration.clone()),
        };
        let mut adder = NetworkAdder {
            expected_path: configuration,
            port,
        };

        let report = execute(
            arguments,
            true,
            &mut AddSelector,
            &mut adder,
            &mut RecordingTransport::default(),
        )
        .await
        .expect("the newly added printer should receive the job");

        assert_eq!(report.printer, "new-printer");
        assert_eq!(
            receiver.join().expect("the receiver should finish"),
            expected
        );
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[test]
    fn several_matching_devices_are_rejected_instead_of_selecting_the_first() {
        let target = UsbTarget {
            vendor_id: 0x0416,
            product_id: 0x5011,
            serial_number: None,
            interface: 0,
            out_endpoint: 0x01,
        };

        let error = require_unique_device(vec!["first", "second"], &target)
            .expect_err("ambiguous devices must fail");

        assert!(matches!(
            error,
            CliError::AmbiguousUsbDevices {
                vendor_id: 0x0416,
                product_id: 0x5011,
                count: 2,
            }
        ));
    }

    #[test]
    fn no_matching_device_is_reported_without_opening_usb() {
        let target = UsbTarget {
            vendor_id: 0x0416,
            product_id: 0x5011,
            serial_number: None,
            interface: 0,
            out_endpoint: 0x01,
        };

        let error = require_unique_device::<()>(Vec::new(), &target)
            .expect_err("a missing device must fail");

        assert!(matches!(
            error,
            CliError::UsbDeviceNotFound {
                vendor_id: 0x0416,
                product_id: 0x5011,
            }
        ));
    }

    #[test]
    fn usb_target_uses_the_conventional_identifier_and_endpoint_notation() {
        let target = UsbTarget {
            vendor_id: 0x0416,
            product_id: 0x5011,
            serial_number: None,
            interface: 0,
            out_endpoint: 0x01,
        };

        assert_eq!(target.to_string(), "0416:5011, interface 0, OUT 0x01");
    }

    #[derive(Default)]
    struct RecordingTransport {
        request: Option<(UsbTarget, Vec<u8>)>,
    }

    struct FixedSelector;
    struct AddSelector;
    struct UnexpectedSelector;
    struct UnexpectedAdder;
    struct NetworkAdder {
        expected_path: PathBuf,
        port: u16,
    }

    impl PrinterSelector for FixedSelector {
        fn select(&mut self, choices: Vec<PrinterChoice>) -> Result<PrinterChoice, CliError> {
            assert_eq!(
                choices,
                vec![
                    PrinterChoice::Printer {
                        name: "counter".to_owned(),
                        transport: "usb",
                        profile: Some("REFERENCE".to_owned()),
                    },
                    PrinterChoice::Add,
                ]
            );
            Ok(choices[0].clone())
        }
    }

    impl PrinterSelector for UnexpectedSelector {
        fn select(&mut self, _choices: Vec<PrinterChoice>) -> Result<PrinterChoice, CliError> {
            panic!("an explicit printer must not open the selector")
        }
    }

    impl PrinterSelector for AddSelector {
        fn select(&mut self, choices: Vec<PrinterChoice>) -> Result<PrinterChoice, CliError> {
            assert_eq!(choices, vec![PrinterChoice::Add]);
            Ok(PrinterChoice::Add)
        }
    }

    impl PrinterAdder for UnexpectedAdder {
        fn add(&mut self, _config_path: Option<&Path>) -> Result<String, CliError> {
            panic!("the add-printer workflow should not run")
        }
    }

    impl PrinterAdder for NetworkAdder {
        fn add(&mut self, config_path: Option<&Path>) -> Result<String, CliError> {
            assert_eq!(config_path, Some(self.expected_path.as_path()));
            fs::write(
                &self.expected_path,
                format!(
                    "\
[new-printer]
transport = \"network\"
host = \"127.0.0.1\"
port = {}
",
                    self.port
                ),
            )
            .expect("the add workflow should update the configuration");
            Ok("new-printer".to_owned())
        }
    }

    impl UsbTransport for RecordingTransport {
        fn send(&mut self, target: &UsbTarget, data: &[u8]) -> Result<(), CliError> {
            self.request = Some((target.clone(), data.to_vec()));
            Ok(())
        }
    }

    fn temporary_directory(case: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock should be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "escpost-print-{case}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("the test directory should be creatable");
        path
    }
}
