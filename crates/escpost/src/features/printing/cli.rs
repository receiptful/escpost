//! Terminal adapter for physical printing.

use std::fmt;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum};
use inquire::Select;

use crate::configuration;
use crate::error::CliError;
use crate::features::printers::cli as printers;
use crate::source;

use super::{Request, ResolveRequest, Target, UsbTarget, print, resolve_target};

#[derive(Debug, Args)]
pub(crate) struct PrintArgs {
    /// Raw ESC/POS file, hexadecimal file, case directory, or - for stdin.
    pub(crate) source: PathBuf,

    /// Input representation.
    #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
    format: InputFormat,

    /// Configured printer name.
    #[arg(long)]
    pub(crate) printer: Option<String>,

    /// Read printer configuration from this exact file.
    #[arg(long, value_name = "FILE")]
    pub(crate) config: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum InputFormat {
    #[default]
    Auto,
    Binary,
    Hex,
}

impl From<InputFormat> for source::InputFormat {
    fn from(format: InputFormat) -> Self {
        match format {
            InputFormat::Auto => Self::Auto,
            InputFormat::Binary => Self::Binary,
            InputFormat::Hex => Self::Hex,
        }
    }
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

trait PrinterSelector {
    fn select(&mut self, choices: Vec<PrinterChoice>) -> Result<PrinterChoice, CliError>;
}

trait PrinterAdder {
    fn add(&mut self, config_path: Option<&Path>) -> Result<String, CliError>;
}

pub(crate) async fn run(arguments: PrintArgs, non_interactive: bool) -> Result<(), CliError> {
    let PrintArgs {
        source,
        format,
        printer,
        config,
    } = arguments;
    let can_prompt = !non_interactive && io::stdin().is_terminal() && io::stderr().is_terminal();
    let printer_name = resolve_printer_name(
        printer,
        config.as_deref(),
        can_prompt,
        &mut InquirePrinterSelector,
        &mut InquirePrinterAdder,
    )?;
    let printer = resolve_target(ResolveRequest {
        printer_name,
        config,
    })?;
    let input = source::load(&source, format.into())?;
    let bytes_sent = input.bytes.len();
    let response = print(Request {
        bytes: input.bytes,
        printer,
    })
    .await?;

    present(&response, bytes_sent);
    Ok(())
}

fn present(response: &super::Response, bytes_sent: usize) {
    eprintln!("Printer: {}", response.printer_name);
    match &response.target {
        Target::Usb(target) => {
            eprintln!("Transport: usb");
            eprintln!("USB target: {}", format_usb_target(target));
        }
        Target::Network(target) => {
            eprintln!("Transport: network");
            eprintln!("Network target: {}", target.endpoint());
        }
    }
    eprintln!("Bytes sent: {bytes_sent}");
}

fn format_usb_target(target: &UsbTarget) -> String {
    let mut output = format!(
        "{:04x}:{:04x}, interface {}, OUT {:#04x}",
        target.vendor_id, target.product_id, target.interface, target.out_endpoint
    );
    if let Some(serial_number) = &target.serial_number {
        output.push_str(", serial ");
        output.push_str(serial_number);
    }
    output
}

#[cfg(test)]
async fn execute(
    arguments: PrintArgs,
    can_prompt: bool,
    selector: &mut impl PrinterSelector,
    adder: &mut impl PrinterAdder,
    transport: &mut impl super::UsbTransport,
) -> Result<super::Response, CliError> {
    let PrintArgs {
        source,
        format,
        printer,
        config,
    } = arguments;
    let printer_name =
        resolve_printer_name(printer, config.as_deref(), can_prompt, selector, adder)?;
    let printer = resolve_target(ResolveRequest {
        printer_name,
        config,
    })?;
    let input = source::load(&source, format.into())?;
    super::print_with_transport(
        Request {
            bytes: input.bytes,
            printer,
        },
        transport,
    )
    .await
    .map_err(Into::into)
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Read;
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        InputFormat, PrintArgs, PrinterAdder, PrinterChoice, PrinterSelector, execute,
        format_usb_target,
    };
    use crate::error::CliError;
    use crate::features::printing::{UsbTarget, UsbTransport};

    #[tokio::test]
    async fn interactive_selection_loads_the_source_and_prints_to_the_chosen_printer() {
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
        let mut transport = RecordingTransport::default();

        let response = execute(
            PrintArgs {
                source,
                format: InputFormat::Auto,
                printer: None,
                config: Some(configuration),
            },
            true,
            &mut FixedSelector,
            &mut UnexpectedAdder,
            &mut transport,
        )
        .await
        .expect("the selected printer should receive the job");

        assert_eq!(response.printer_name, "counter");
        assert_eq!(
            transport.request.expect("USB should receive the job").1,
            vec![0x1b, 0x40, 0x0a]
        );
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[tokio::test]
    async fn interactive_addition_loads_the_source_and_prints_to_the_new_printer() {
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
        let mut adder = NetworkAdder {
            expected_path: configuration.clone(),
            port,
        };

        let response = execute(
            PrintArgs {
                source,
                format: InputFormat::Auto,
                printer: None,
                config: Some(configuration),
            },
            true,
            &mut AddSelector,
            &mut adder,
            &mut RecordingTransport::default(),
        )
        .await
        .expect("the newly added printer should receive the job");

        assert_eq!(response.printer_name, "new-printer");
        assert_eq!(
            receiver.join().expect("the receiver should finish"),
            expected
        );
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[test]
    fn usb_target_uses_the_conventional_identifier_and_endpoint_notation() {
        assert_eq!(
            format_usb_target(&UsbTarget {
                vendor_id: 0x0416,
                product_id: 0x5011,
                serial_number: None,
                interface: 0,
                out_endpoint: 0x01,
            }),
            "0416:5011, interface 0, OUT 0x01"
        );
    }

    #[derive(Default)]
    struct RecordingTransport {
        request: Option<(UsbTarget, Vec<u8>)>,
    }

    struct FixedSelector;
    struct AddSelector;
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

    impl PrinterAdder for UnexpectedAdder {
        fn add(&mut self, _config_path: Option<&Path>) -> Result<String, CliError> {
            panic!("the add-printer workflow should not run")
        }
    }

    impl PrinterSelector for AddSelector {
        fn select(&mut self, choices: Vec<PrinterChoice>) -> Result<PrinterChoice, CliError> {
            assert_eq!(choices, vec![PrinterChoice::Add]);
            Ok(PrinterChoice::Add)
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
        fn send(&mut self, target: &UsbTarget, data: &[u8]) -> crate::application::Result<()> {
            self.request = Some((target.clone(), data.to_vec()));
            Ok(())
        }
    }

    fn temporary_directory(case: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the system clock should be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "escpost-printing-cli-{case}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("the test directory should be creatable");
        path
    }
}
