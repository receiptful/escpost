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
    let can_prompt = !non_interactive && io::stdin().is_terminal() && io::stderr().is_terminal();
    let request = prepare_request(
        arguments,
        can_prompt,
        &mut InquirePrinterSelector,
        &mut InquirePrinterAdder,
    )?;
    let response = print(request).await?;

    present(&response);
    Ok(())
}

fn prepare_request(
    arguments: PrintArgs,
    can_prompt: bool,
    selector: &mut impl PrinterSelector,
    adder: &mut impl PrinterAdder,
) -> Result<Request, CliError> {
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
    Ok(Request {
        bytes: input.bytes,
        printer,
    })
}

fn present(response: &super::Response) {
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
    eprintln!("Bytes sent: {}", response.bytes_sent);
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
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        InputFormat, PrintArgs, PrinterAdder, PrinterChoice, PrinterSelector, format_usb_target,
        prepare_request,
    };
    use crate::application::ApplicationError;
    use crate::error::CliError;
    use crate::features::printing::{NetworkTarget, Target, UsbTarget};

    #[test]
    fn interactive_selection_prepares_the_chosen_target_and_source_bytes() {
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
        let request = prepare_request(
            PrintArgs {
                source,
                format: InputFormat::Auto,
                printer: None,
                config: Some(configuration),
            },
            true,
            &mut FixedSelector,
            &mut UnexpectedAdder,
        )
        .expect("the selected printer request should be prepared");

        assert_eq!(request.bytes, vec![0x1b, 0x40, 0x0a]);
        assert_eq!(request.printer.printer_name, "counter");
        assert_eq!(
            request.printer.target,
            Target::Usb(UsbTarget {
                vendor_id: 0x0416,
                product_id: 0x5011,
                serial_number: None,
                interface: 0,
                out_endpoint: 0x01,
            })
        );
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[test]
    fn interactive_addition_prepares_the_new_target_and_source_bytes() {
        let directory = temporary_directory("interactive-add");
        let source = directory.join("receipt.bin");
        let configuration = directory.join("printers.toml");
        let expected = b"\x1b@New printer\n";
        fs::write(&source, expected).expect("the source should be writable");
        let mut adder = NetworkAdder {
            expected_path: configuration.clone(),
            port: 9123,
        };

        let request = prepare_request(
            PrintArgs {
                source,
                format: InputFormat::Auto,
                printer: None,
                config: Some(configuration),
            },
            true,
            &mut AddSelector,
            &mut adder,
        )
        .expect("the newly added printer request should be prepared");

        assert_eq!(request.bytes, expected);
        assert_eq!(request.printer.printer_name, "new-printer");
        assert_eq!(
            request.printer.target,
            Target::Network(NetworkTarget {
                host: "127.0.0.1".to_owned(),
                port: 9123,
            })
        );
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[test]
    fn named_target_resolution_precedes_source_loading() {
        let directory = temporary_directory("target-before-source");
        let configuration = directory.join("printers.toml");
        fs::write(
            &configuration,
            "\
[counter]
transport = \"network\"
host = \"127.0.0.1\"
port = 9100
",
        )
        .expect("the printer configuration should be writable");

        let error = prepare_request(
            PrintArgs {
                source: directory.join("missing.hex"),
                format: InputFormat::Auto,
                printer: Some("missing".to_owned()),
                config: Some(configuration),
            },
            true,
            &mut UnexpectedSelector,
            &mut UnexpectedAdder,
        )
        .err()
        .expect("the unknown target should fail before the missing source is loaded");

        assert!(matches!(
            error,
            CliError::Application(ApplicationError::UnknownConfiguredPrinter(name))
                if name == "missing"
        ));
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

    impl PrinterSelector for UnexpectedSelector {
        fn select(&mut self, _choices: Vec<PrinterChoice>) -> Result<PrinterChoice, CliError> {
            panic!("an explicit printer should not prompt for selection")
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
