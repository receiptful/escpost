use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::application::{self, ApplicationError};

const CONFIG_DIRECTORY_ENV: &str = "ESCPOST_CONFIG_DIR";
const CONFIG_DISPLAY_DIRECTORY_ENV: &str = "ESCPOST_CONFIG_DISPLAY_DIR";
const PRINTERS_FILE: &str = "printers.toml";
static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Default)]
pub(crate) struct PrinterConfiguration {
    usb_printers: Vec<ConfiguredUsbPrinter>,
    network_printers: Vec<ConfiguredNetworkPrinter>,
}

#[derive(Debug)]
pub(crate) struct ConfiguredUsbPrinter {
    pub(crate) name: String,
    pub(crate) profile: Option<String>,
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) serial_number: Option<String>,
    pub(crate) interface_number: u8,
    pub(crate) out_endpoint: u8,
    pub(crate) in_endpoint: Option<u8>,
}

#[derive(Debug)]
pub(crate) struct ConfiguredNetworkPrinter {
    pub(crate) name: String,
    pub(crate) profile: Option<String>,
    pub(crate) host: String,
    pub(crate) port: u16,
}

pub(crate) struct UsbPrinterRegistration<'a> {
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) serial_number: Option<&'a str>,
    pub(crate) interface_number: u8,
    pub(crate) out_endpoint: u8,
    pub(crate) in_endpoint: Option<u8>,
    pub(crate) profile: Option<&'a str>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ConfiguredPrinter<'a> {
    Usb(&'a ConfiguredUsbPrinter),
    Network(&'a ConfiguredNetworkPrinter),
}

impl PrinterConfiguration {
    pub(crate) fn parse(content: &str) -> Result<Self, String> {
        let document = toml::from_str::<toml::Table>(content).map_err(|error| error.to_string())?;
        let mut usb_printers = Vec::new();
        let mut network_printers = Vec::new();

        for (name, value) in document {
            let table = value
                .as_table()
                .ok_or_else(|| format!("printer {name:?} must be a table"))?;
            let transport = required_string(table, "transport", &name)?;
            if transport == "network" {
                network_printers.push(parse_network_printer(table, name)?);
                continue;
            }
            if transport == "usb" {
                usb_printers.push(ConfiguredUsbPrinter {
                    profile: optional_string(table, "profile", &name)?,
                    vendor_id: required_integer(table, "vendor_id", &name)?,
                    product_id: required_integer(table, "product_id", &name)?,
                    serial_number: optional_string(table, "serial_number", &name)?,
                    interface_number: required_integer(table, "interface_number", &name)?,
                    out_endpoint: required_integer(table, "out_endpoint", &name)?,
                    in_endpoint: optional_integer(table, "in_endpoint", &name)?,
                    name,
                });
            }
        }

        Ok(Self {
            usb_printers,
            network_printers,
        })
    }

    pub(crate) fn usb_printers(&self) -> &[ConfiguredUsbPrinter] {
        &self.usb_printers
    }

    pub(crate) fn network_printers(&self) -> &[ConfiguredNetworkPrinter] {
        &self.network_printers
    }

    pub(crate) fn printer(&self, name: &str) -> Option<ConfiguredPrinter<'_>> {
        self.usb_printers
            .iter()
            .find(|printer| printer.name == name)
            .map(ConfiguredPrinter::Usb)
            .or_else(|| {
                self.network_printers
                    .iter()
                    .find(|printer| printer.name == name)
                    .map(ConfiguredPrinter::Network)
            })
    }

    pub(crate) fn printers(&self) -> impl Iterator<Item = ConfiguredPrinter<'_>> {
        self.usb_printers
            .iter()
            .map(ConfiguredPrinter::Usb)
            .chain(self.network_printers.iter().map(ConfiguredPrinter::Network))
    }
}

impl ConfiguredPrinter<'_> {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Usb(printer) => &printer.name,
            Self::Network(printer) => &printer.name,
        }
    }

    pub(crate) fn transport(&self) -> &'static str {
        match self {
            Self::Usb(_) => "usb",
            Self::Network(_) => "network",
        }
    }

    pub(crate) fn profile(&self) -> Option<&str> {
        match self {
            Self::Usb(printer) => printer.profile.as_deref(),
            Self::Network(printer) => printer.profile.as_deref(),
        }
    }
}

/// Load the selected printer configuration when it exists.
///
/// Missing implicit configuration is normal, but a file named explicitly by
/// the developer must be readable and valid. Keeping that distinction here
/// prevents read-only commands from creating configuration as a side effect.
pub(crate) fn load(explicit_path: Option<&Path>) -> application::Result<PrinterConfiguration> {
    let (path, required) = resolve_path(explicit_path)?;

    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(source) if !required && source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PrinterConfiguration::default());
        }
        Err(source) => {
            return Err(ApplicationError::ReadPrinterConfiguration { path, source });
        }
    };
    PrinterConfiguration::parse(&content)
        .map_err(|message| ApplicationError::InvalidPrinterConfiguration { path, message })
}

/// Load configuration before a command which may create the selected file.
pub(crate) fn load_for_update(
    explicit_path: Option<&Path>,
) -> application::Result<PrinterConfiguration> {
    let (path, _) = resolve_path(explicit_path)?;
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PrinterConfiguration::default());
        }
        Err(source) => return Err(ApplicationError::ReadPrinterConfiguration { path, source }),
    };
    PrinterConfiguration::parse(&content)
        .map_err(|message| ApplicationError::InvalidPrinterConfiguration { path, message })
}

pub(crate) fn add_network_printer(
    explicit_path: Option<&Path>,
    name: &str,
    host: &str,
    port: u16,
    profile: Option<&str>,
) -> application::Result<PathBuf> {
    let mut printer = toml::Table::new();
    printer.insert(
        "transport".to_owned(),
        toml::Value::String("network".to_owned()),
    );
    printer.insert("host".to_owned(), toml::Value::String(host.to_owned()));
    printer.insert("port".to_owned(), toml::Value::Integer(i64::from(port)));
    if let Some(profile) = profile {
        printer.insert(
            "profile".to_owned(),
            toml::Value::String(profile.to_owned()),
        );
    }
    add_printer_table(explicit_path, name, printer)
}

pub(crate) fn add_usb_printer(
    explicit_path: Option<&Path>,
    name: &str,
    registration: &UsbPrinterRegistration<'_>,
) -> application::Result<PathBuf> {
    let mut printer = toml::Table::new();
    printer.insert(
        "transport".to_owned(),
        toml::Value::String("usb".to_owned()),
    );
    printer.insert(
        "vendor_id".to_owned(),
        toml::Value::String(format!("{:#06x}", registration.vendor_id)),
    );
    printer.insert(
        "product_id".to_owned(),
        toml::Value::String(format!("{:#06x}", registration.product_id)),
    );
    if let Some(serial_number) = registration.serial_number {
        printer.insert(
            "serial_number".to_owned(),
            toml::Value::String(serial_number.to_owned()),
        );
    }
    printer.insert(
        "interface_number".to_owned(),
        toml::Value::Integer(i64::from(registration.interface_number)),
    );
    printer.insert(
        "out_endpoint".to_owned(),
        toml::Value::String(format!("{:#04x}", registration.out_endpoint)),
    );
    if let Some(in_endpoint) = registration.in_endpoint {
        printer.insert(
            "in_endpoint".to_owned(),
            toml::Value::String(format!("{in_endpoint:#04x}")),
        );
    }
    if let Some(profile) = registration.profile {
        printer.insert(
            "profile".to_owned(),
            toml::Value::String(profile.to_owned()),
        );
    }
    add_printer_table(explicit_path, name, printer)
}

fn add_printer_table(
    explicit_path: Option<&Path>,
    name: &str,
    printer: toml::Table,
) -> application::Result<PathBuf> {
    let (path, _) = resolve_path(explicit_path)?;
    let existing = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => return Err(ApplicationError::ReadPrinterConfiguration { path, source }),
    };
    if !existing.is_empty() {
        PrinterConfiguration::parse(&existing).map_err(|message| {
            ApplicationError::InvalidPrinterConfiguration {
                path: path.clone(),
                message,
            }
        })?;
    }
    let document = if existing.is_empty() {
        toml::Table::new()
    } else {
        toml::from_str::<toml::Table>(&existing).map_err(|error| {
            ApplicationError::InvalidPrinterConfiguration {
                path: path.clone(),
                message: error.to_string(),
            }
        })?
    };
    if document.contains_key(name) {
        return Err(ApplicationError::PrinterAlreadyConfigured(name.to_owned()));
    }

    // Serialize only the new table, then append it to the original text. This
    // keeps comments, field order, and formatting chosen by developers.
    let mut addition = toml::Table::new();
    addition.insert(name.to_owned(), toml::Value::Table(printer));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            ApplicationError::CreatePrinterConfigurationDirectory {
                path: parent.to_owned(),
                source,
            }
        })?;
    }
    let addition = toml::to_string_pretty(&addition)
        .map_err(|error| ApplicationError::SerializePrinterConfiguration(error.to_string()))?;
    let mut content = existing;
    if !content.is_empty() {
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push('\n');
    }
    content.push_str(&addition);
    write_atomically(&path, content.as_bytes()).map_err(|source| {
        ApplicationError::WritePrinterConfiguration {
            path: path.clone(),
            source,
        }
    })?;
    Ok(path)
}

/// Replace a configuration only after its complete new contents are written.
///
/// The temporary file lives beside the destination, so the final rename stays
/// on one filesystem. An interrupted process may leave an unused temporary
/// file, but the destination remains either the old or complete new document.
fn write_atomically(path: &Path, content: &[u8]) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(PRINTERS_FILE))
        .to_string_lossy();
    let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        sequence
    ));

    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);

        let mut file = options.open(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);

        if let Ok(metadata) = fs::metadata(path) {
            fs::set_permissions(&temporary, metadata.permissions())?;
        }
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Report the configuration path a command reads or writes, without requiring
/// the file to exist. Read-only commands use this to tell a developer where
/// printers are configured.
pub(crate) fn resolved_path(explicit_path: Option<&Path>) -> application::Result<PathBuf> {
    resolve_path(explicit_path).map(|(path, _)| path)
}

fn resolve_path(explicit_path: Option<&Path>) -> application::Result<(PathBuf, bool)> {
    match explicit_path {
        Some(path) => Ok((path.to_owned(), true)),
        None => match config_directory_override() {
            Some(directory) => Ok((directory.join(PRINTERS_FILE), false)),
            None => Ok((platform_config_directory()?.join(PRINTERS_FILE), false)),
        },
    }
}

fn config_directory_override() -> Option<PathBuf> {
    env::var_os(CONFIG_DIRECTORY_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Render a configuration path for human output.
///
/// The development Docker wrapper mounts a host directory at the container's
/// conventional configuration path, so a resolved path names a location that
/// does not exist on the host. When the wrapper records the backing host
/// directory, map the configuration directory back to it; otherwise show the
/// path unchanged.
pub(crate) fn display_path(path: &Path) -> String {
    display_path_with(
        path,
        config_directory_override().as_deref(),
        config_display_directory().as_deref(),
    )
}

fn config_display_directory() -> Option<PathBuf> {
    env::var_os(CONFIG_DISPLAY_DIRECTORY_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn display_path_with(
    path: &Path,
    config_directory: Option<&Path>,
    display_directory: Option<&Path>,
) -> String {
    if let (Some(config_directory), Some(display_directory)) = (config_directory, display_directory)
        && let Ok(relative) = path.strip_prefix(config_directory)
    {
        return display_directory.join(relative).display().to_string();
    }
    path.display().to_string()
}

fn platform_config_directory() -> application::Result<PathBuf> {
    directories::ProjectDirs::from("io", "receiptful", "escpost")
        .map(|directories| directories.config_dir().to_owned())
        .ok_or(ApplicationError::NoUserConfigDirectory)
}

fn parse_network_printer(
    table: &toml::Table,
    name: String,
) -> Result<ConfiguredNetworkPrinter, String> {
    let host = required_string(table, "host", &name)?.to_owned();
    let port = required_integer::<u16>(table, "port", &name)?;
    if port == 0 {
        return Err(format!(
            "printer {name:?} field \"port\" must be between 1 and 65535"
        ));
    }
    Ok(ConfiguredNetworkPrinter {
        profile: optional_string(table, "profile", &name)?,
        name,
        host,
        port,
    })
}

fn required_string<'a>(
    table: &'a toml::Table,
    field: &str,
    printer: &str,
) -> Result<&'a str, String> {
    table
        .get(field)
        .and_then(toml::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("printer {printer:?} field {field:?} must be a non-empty string"))
}

fn optional_string(
    table: &toml::Table,
    field: &str,
    printer: &str,
) -> Result<Option<String>, String> {
    match table.get(field) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .filter(|value| !value.is_empty())
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| {
                format!("printer {printer:?} field {field:?} must be a non-empty string")
            }),
    }
}

fn required_integer<T>(table: &toml::Table, field: &str, printer: &str) -> Result<T, String>
where
    T: TryFrom<u64>,
{
    let value = table
        .get(field)
        .ok_or_else(|| format!("printer {printer:?} is missing field {field:?}"))?;
    let integer = match value {
        toml::Value::Integer(value) => u64::try_from(*value).ok(),
        toml::Value::String(value) => parse_integer_string(value),
        _ => None,
    }
    .ok_or_else(|| format!("printer {printer:?} field {field:?} must be a non-negative integer"))?;

    T::try_from(integer).map_err(|_| format!("printer {printer:?} field {field:?} is out of range"))
}

fn optional_integer<T>(table: &toml::Table, field: &str, printer: &str) -> Result<Option<T>, String>
where
    T: TryFrom<u64>,
{
    table
        .get(field)
        .map(|_| required_integer(table, field, printer).map(Some))
        .unwrap_or(Ok(None))
}

fn parse_integer_string(value: &str) -> Option<u64> {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(
            || value.parse().ok(),
            |digits| u64::from_str_radix(digits, 16).ok(),
        )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::display_path_with;

    #[test]
    fn a_path_inside_the_config_directory_is_shown_under_the_host_directory() {
        let display = display_path_with(
            Path::new("/home/developer/.config/escpost/printers.toml"),
            Some(Path::new("/home/developer/.config/escpost")),
            Some(Path::new("/checkout/.config")),
        );

        assert_eq!(display, "/checkout/.config/printers.toml");
    }

    #[test]
    fn a_path_outside_the_config_directory_is_shown_unchanged() {
        let display = display_path_with(
            Path::new("/tmp/explicit/printers.toml"),
            Some(Path::new("/home/developer/.config/escpost")),
            Some(Path::new("/checkout/.config")),
        );

        assert_eq!(display, "/tmp/explicit/printers.toml");
    }

    #[test]
    fn without_a_host_directory_the_path_is_shown_unchanged() {
        let display = display_path_with(
            Path::new("/home/developer/.config/escpost/printers.toml"),
            Some(Path::new("/home/developer/.config/escpost")),
            None,
        );

        assert_eq!(display, "/home/developer/.config/escpost/printers.toml");
    }
}
