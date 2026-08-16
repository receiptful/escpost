use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::application::{self, ApplicationError};

const CONFIG_DIRECTORY_ENV: &str = "ESCPOST_CONFIG_DIR";
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
    mutate_configuration(explicit_path, move |path, existing| {
        if !existing.is_empty() {
            PrinterConfiguration::parse(existing).map_err(|message| {
                ApplicationError::InvalidPrinterConfiguration {
                    path: path.to_owned(),
                    message,
                }
            })?;
        }
        let document = if existing.is_empty() {
            toml::Table::new()
        } else {
            toml::from_str::<toml::Table>(existing).map_err(|error| {
                ApplicationError::InvalidPrinterConfiguration {
                    path: path.to_owned(),
                    message: error.to_string(),
                }
            })?
        };
        if document.contains_key(name) {
            return Err(ApplicationError::PrinterAlreadyConfigured(name.to_owned()));
        }

        // Serialize only the new table, then append it to the original text.
        // This keeps comments, field order, and formatting chosen by developers.
        let mut addition = toml::Table::new();
        addition.insert(name.to_owned(), toml::Value::Table(printer));
        let addition = toml::to_string_pretty(&addition)
            .map_err(|error| ApplicationError::SerializePrinterConfiguration(error.to_string()))?;
        let mut content = existing.to_owned();
        if !content.is_empty() {
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push('\n');
        }
        content.push_str(&addition);
        Ok(content)
    })
}

/// Mutate the complete printer configuration under an inter-process lock.
///
/// The stable sibling lock file is never removed. The operating system releases
/// the advisory lock when this process closes the file or exits unexpectedly.
fn mutate_configuration<F>(
    explicit_path: Option<&Path>,
    mutation: F,
) -> application::Result<PathBuf>
where
    F: FnOnce(&Path, &str) -> application::Result<String>,
{
    let (path, _) = resolve_path(explicit_path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| {
        ApplicationError::CreatePrinterConfigurationDirectory {
            path: parent.to_owned(),
            source,
        }
    })?;

    let file_name = path
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new(PRINTERS_FILE))
        .to_string_lossy();
    let lock_path = parent.join(format!(".{file_name}.lock"));
    let mut lock_options = OpenOptions::new();
    lock_options.read(true).write(true).create(true);
    #[cfg(unix)]
    lock_options.mode(0o600);
    let lock_file = lock_options.open(&lock_path).map_err(|source| {
        ApplicationError::OpenPrinterConfigurationLock {
            path: lock_path.clone(),
            source,
        }
    })?;
    lock_file
        .lock()
        .map_err(|source| ApplicationError::LockPrinterConfiguration {
            path: lock_path,
            source,
        })?;

    let existing = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(ApplicationError::ReadPrinterConfiguration {
                path: path.clone(),
                source,
            });
        }
    };
    let content = mutation(&path, &existing)?;
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
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::mutate_configuration;

    #[test]
    fn concurrent_mutations_see_the_latest_complete_configuration() {
        let directory =
            std::env::temp_dir().join(format!("escpost-locked-mutation-{}", std::process::id()));
        let path = directory.join("printers.toml");
        std::fs::create_dir_all(&directory).expect("temporary directory should be creatable");
        std::fs::write(&path, "# original\n").expect("fixture should be writable");

        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first_path = path.clone();
        let first = thread::spawn(move || {
            mutate_configuration(Some(&first_path), |_path, existing| {
                first_entered_tx
                    .send(())
                    .expect("test receiver should exist");
                release_first_rx
                    .recv()
                    .expect("first mutation should resume");
                Ok(format!("{existing}# first\n"))
            })
        });
        first_entered_rx
            .recv()
            .expect("first mutation should hold the lock");

        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let (second_started_tx, second_started_rx) = mpsc::channel();
        let second_path = path.clone();
        let second = thread::spawn(move || {
            second_started_tx
                .send(())
                .expect("test receiver should exist");
            mutate_configuration(Some(&second_path), |_path, existing| {
                second_entered_tx
                    .send(existing.to_owned())
                    .expect("test receiver should exist");
                Ok(format!("{existing}# second\n"))
            })
        });
        second_started_rx
            .recv()
            .expect("second mutation should attempt to acquire the lock");

        assert!(
            second_entered_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "the second mutation must wait until the first releases the file lock"
        );
        release_first_tx
            .send(())
            .expect("first mutation should still be waiting");
        first.join().expect("first thread should finish").unwrap();
        let content_seen_by_second = second_entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("second mutation should proceed after the first");
        second.join().expect("second thread should finish").unwrap();

        assert_eq!(content_seen_by_second, "# original\n# first\n");
        assert_eq!(
            std::fs::read_to_string(&path).expect("result should be readable"),
            "# original\n# first\n# second\n"
        );
        std::fs::remove_dir_all(directory).expect("temporary directory should be removable");
    }
}
