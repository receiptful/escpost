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

use super::{
    NetworkTarget, Request, ResolveRequest, ResolvedPrinter, Target, UsbTarget, print,
    resolve_target,
};

const DEFAULT_NETWORK_HOST: &str = "127.0.0.1";
const DEFAULT_NETWORK_PORT: u16 = 9100;

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

    /// Send directly to a RAW TCP endpoint; a bare port uses 127.0.0.1 and a bare host uses port 9100.
    #[arg(
        long,
        value_name = "PORT|HOST|HOST:PORT",
        value_parser = parse_network_target,
        conflicts_with_all = ["printer", "config"]
    )]
    pub(crate) network: Option<NetworkTarget>,
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

fn invalid_network_target(value: &str) -> String {
    format!(
        "invalid network target {value:?}; expected PORT, HOST, HOST:PORT, [IPv6], or [IPv6]:PORT"
    )
}

fn parse_network_port(value: &str, original: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| invalid_network_target(original))
}

fn parse_network_target(value: &str) -> Result<NetworkTarget, String> {
    if value.is_empty()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(invalid_network_target(value));
    }

    if value.chars().all(|character| character.is_ascii_digit()) {
        return Ok(NetworkTarget {
            host: DEFAULT_NETWORK_HOST.to_owned(),
            port: parse_network_port(value, value)?,
        });
    }

    if let Some(remainder) = value.strip_prefix('[') {
        let Some((host, suffix)) = remainder.split_once(']') else {
            return Err(invalid_network_target(value));
        };
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(invalid_network_target(value));
        }
        let port = match suffix {
            "" => DEFAULT_NETWORK_PORT,
            _ => parse_network_port(
                suffix
                    .strip_prefix(':')
                    .ok_or_else(|| invalid_network_target(value))?,
                value,
            )?,
        };
        return Ok(NetworkTarget {
            host: host.to_owned(),
            port,
        });
    }

    if value.contains(['[', ']']) {
        return Err(invalid_network_target(value));
    }

    match value.matches(':').count() {
        0 => Ok(NetworkTarget {
            host: value.to_owned(),
            port: DEFAULT_NETWORK_PORT,
        }),
        1 => {
            let Some((host, port)) = value.split_once(':') else {
                return Err(invalid_network_target(value));
            };
            if host.is_empty() || port.is_empty() {
                return Err(invalid_network_target(value));
            }
            Ok(NetworkTarget {
                host: host.to_owned(),
                port: parse_network_port(port, value)?,
            })
        }
        _ => Err(invalid_network_target(value)),
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
        network,
    } = arguments;
    let printer = match network {
        Some(target) => ResolvedPrinter::direct_network(target),
        None => {
            let printer_name =
                resolve_printer_name(printer, config.as_deref(), can_prompt, selector, adder)?;
            resolve_target(ResolveRequest {
                printer_name,
                config,
            })?
        }
    };
    let input = source::load(&source, format.into())?;
    Ok(Request {
        bytes: input.bytes,
        printer,
    })
}

fn present(response: &super::Response) {
    if let Some(printer_name) = &response.printer_name {
        eprintln!("Printer: {printer_name}");
    }
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

    use clap::Parser;

    use super::{
        InputFormat, PrintArgs, PrinterAdder, PrinterChoice, PrinterSelector, format_usb_target,
        parse_network_target, prepare_request,
    };
    use crate::application::ApplicationError;
    use crate::cli::Cli;
    use crate::error::CliError;
    use crate::features::printing::{NetworkTarget, Target, UsbTarget};

    #[test]
    fn direct_network_target_accepts_shorthand_and_explicit_endpoints() {
        for (input, host, port) in [
            ("9100", "127.0.0.1", 9100),
            ("printer.local", "printer.local", 9100),
            ("printer.local:9200", "printer.local", 9200),
            ("[::1]", "::1", 9100),
            ("[::1]:9200", "::1", 9200),
        ] {
            assert_eq!(
                parse_network_target(input).expect("the endpoint should parse"),
                NetworkTarget {
                    host: host.to_owned(),
                    port,
                },
                "input {input:?}",
            );
        }
    }

    #[test]
    fn direct_network_target_rejects_ambiguous_or_invalid_endpoints() {
        for input in [
            "",
            "0",
            "65536",
            ":9100",
            "printer.local:",
            "[::1",
            "[::1]:0",
            "::1",
        ] {
            let message = parse_network_target(input).expect_err("the endpoint should be rejected");
            assert!(
                message.contains(input),
                "the error should name {input:?}: {message}"
            );
        }
    }

    #[test]
    fn direct_network_target_rejects_whitespace_and_control_characters_in_hosts() {
        for (input, expected) in [
            (
                "printer local",
                "invalid network target \"printer local\"; expected PORT, HOST, HOST:PORT, [IPv6], or [IPv6]:PORT",
            ),
            (
                "printer\tlocal",
                "invalid network target \"printer\\tlocal\"; expected PORT, HOST, HOST:PORT, [IPv6], or [IPv6]:PORT",
            ),
            (
                "printer\nlocal",
                "invalid network target \"printer\\nlocal\"; expected PORT, HOST, HOST:PORT, [IPv6], or [IPv6]:PORT",
            ),
            (
                "printer\u{7f}local",
                "invalid network target \"printer\\u{7f}local\"; expected PORT, HOST, HOST:PORT, [IPv6], or [IPv6]:PORT",
            ),
        ] {
            assert_eq!(
                parse_network_target(input).expect_err("the endpoint should be rejected"),
                expected,
                "input {input:?}",
            );
        }
    }

    #[test]
    fn malformed_direct_endpoint_is_rejected_during_cli_parsing_before_source_loading() {
        let error = Cli::try_parse_from([
            "escpost",
            "print",
            "definitely-missing.hex",
            "--network",
            "printer local",
        ])
        .expect_err("the malformed endpoint should fail CLI parsing");
        let message = error.to_string();

        assert!(message.contains("invalid network target \"printer local\""));
        assert!(!message.contains("ESC/POS input file"));
    }

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
                network: None,
            },
            true,
            &mut FixedSelector,
            &mut UnexpectedAdder,
        )
        .expect("the selected printer request should be prepared");

        assert_eq!(request.bytes, vec![0x1b, 0x40, 0x0a]);
        assert_eq!(request.printer.printer_name, Some("counter".to_owned()));
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
                network: None,
            },
            true,
            &mut AddSelector,
            &mut adder,
        )
        .expect("the newly added printer request should be prepared");

        assert_eq!(request.bytes, expected);
        assert_eq!(request.printer.printer_name, Some("new-printer".to_owned()));
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
                network: None,
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
