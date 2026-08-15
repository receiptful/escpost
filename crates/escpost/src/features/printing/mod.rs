//! Typed physical-printer operation for already-loaded ESC/POS bytes.

use std::fmt;
use std::io::Write;
use std::time::Duration;

use nusb::MaybeFuture;
use nusb::transfer::{Bulk, Out};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::application;
use crate::configuration::{self, ConfiguredPrinter};
use crate::error::CliError;

pub(crate) mod cli;

const USB_WRITE_BUFFER_BYTES: usize = 16 * 1024;
const USB_TRANSFER_TIMEOUT: Duration = Duration::from_secs(10);
const NETWORK_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const NETWORK_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// An explicit physical-print request. The bytes are already in ESC/POS wire
/// format and are transmitted unchanged.
pub(crate) struct Request {
    pub(crate) bytes: Vec<u8>,
    pub(crate) printer_name: String,
    pub(crate) config: Option<std::path::PathBuf>,
}

/// Facts about the target selected from printer configuration.
pub(crate) struct Response {
    pub(crate) printer_name: String,
    pub(crate) target: Target,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Usb(UsbTarget),
    Network(NetworkTarget),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UsbTarget {
    vendor_id: u16,
    product_id: u16,
    serial_number: Option<String>,
    interface: u8,
    out_endpoint: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NetworkTarget {
    host: String,
    port: u16,
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

impl fmt::Display for NetworkTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.host.contains(':') && !(self.host.starts_with('[') && self.host.ends_with(']')) {
            write!(formatter, "[{}]:{}", self.host, self.port)
        } else {
            write!(formatter, "{}:{}", self.host, self.port)
        }
    }
}

trait UsbTransport {
    fn send(&mut self, target: &UsbTarget, data: &[u8]) -> application::Result<()>;
}

/// Resolve a named configured target and transmit the caller's exact bytes.
///
/// This operation is deliberately presentation-free: callers choose names,
/// load sources, and render the resulting target facts for their own transport.
pub(crate) async fn print(request: Request) -> application::Result<Response> {
    print_with_transport(request, &mut NusbTransport).await
}

async fn print_with_transport(
    request: Request,
    transport: &mut impl UsbTransport,
) -> application::Result<Response> {
    let configuration = configuration::load(request.config.as_deref())?;
    let printer = configuration
        .printer(&request.printer_name)
        .ok_or_else(|| CliError::UnknownConfiguredPrinter(request.printer_name.clone()))?;
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
            transport.send(&target, &request.bytes)?;
            Target::Usb(target)
        }
        ConfiguredPrinter::Network(printer) => {
            let target = NetworkTarget {
                host: printer.host.clone(),
                port: printer.port,
            };
            send_network(&target, &request.bytes).await?;
            Target::Network(target)
        }
    };

    Ok(Response {
        printer_name: request.printer_name,
        target,
    })
}

async fn send_network(target: &NetworkTarget, data: &[u8]) -> application::Result<()> {
    let endpoint = target.to_string();
    let mut stream = timeout(
        NETWORK_CONNECT_TIMEOUT,
        TcpStream::connect((target.host.as_str(), target.port)),
    )
    .await
    .map_err(|_| CliError::ConnectNetworkPrinterTimeout(endpoint.clone()))?
    .map_err(|source| CliError::ConnectNetworkPrinter {
        target: endpoint.clone(),
        source,
    })?;

    timeout(NETWORK_WRITE_TIMEOUT, async {
        stream.write_all(data).await?;
        stream.shutdown().await
    })
    .await
    .map_err(|_| CliError::WriteNetworkPrinterTimeout(endpoint.clone()))?
    .map_err(|source| CliError::WriteNetworkPrinter {
        target: endpoint,
        source,
    })
}

struct NusbTransport;

impl UsbTransport for NusbTransport {
    fn send(&mut self, target: &UsbTarget, data: &[u8]) -> application::Result<()> {
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

fn require_unique_device<T>(mut matches: Vec<T>, target: &UsbTarget) -> application::Result<T> {
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
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        NetworkTarget, Request, Target, UsbTarget, UsbTransport, print, print_with_transport,
        require_unique_device,
    };
    use crate::error::CliError;

    #[tokio::test]
    async fn exact_bytes_reach_the_named_usb_target_unchanged() {
        let directory = temporary_directory("exact-usb");
        let configuration = directory.join("printers.toml");
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
        let mut transport = RecordingTransport::default();

        let response = print_with_transport(
            Request {
                bytes: vec![0x1b, 0x40, 0x00, 0xff, 0x0a],
                printer_name: "counter".to_owned(),
                config: Some(configuration),
            },
            &mut transport,
        )
        .await
        .expect("printing should succeed");

        assert_eq!(response.printer_name, "counter");
        assert_eq!(
            response.target,
            Target::Usb(UsbTarget {
                vendor_id: 0x0416,
                product_id: 0x5011,
                serial_number: Some("B120300001".to_owned()),
                interface: 0,
                out_endpoint: 0x01,
            })
        );
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
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[tokio::test]
    async fn exact_bytes_reach_the_named_network_target_unchanged() {
        let directory = temporary_directory("exact-network");
        let configuration = directory.join("printers.toml");
        let listener =
            TcpListener::bind(("127.0.0.1", 0)).expect("the loopback printer should bind");
        let port = listener
            .local_addr()
            .expect("the listener should have an address")
            .port();
        fs::write(
            &configuration,
            format!(
                "\
[kitchen]
transport = \"network\"
host = \"127.0.0.1\"
port = {port}
"
            ),
        )
        .expect("the printer configuration should be writable");
        let receiver = thread::spawn(move || {
            let (mut connection, _) = listener
                .accept()
                .expect("the printer should receive a connection");
            let mut bytes = Vec::new();
            connection
                .read_to_end(&mut bytes)
                .expect("the print connection should close cleanly");
            bytes
        });

        let response = print(Request {
            bytes: vec![0x1b, b'@', 0x00, 0xff, b'\n'],
            printer_name: "kitchen".to_owned(),
            config: Some(configuration),
        })
        .await
        .expect("printing should succeed");

        assert_eq!(response.printer_name, "kitchen");
        assert_eq!(
            response.target,
            Target::Network(NetworkTarget {
                host: "127.0.0.1".to_owned(),
                port,
            })
        );
        assert_eq!(
            receiver.join().expect("the receiver should finish"),
            vec![0x1b, b'@', 0x00, 0xff, b'\n']
        );
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[test]
    fn several_matching_devices_are_rejected_instead_of_selecting_the_first() {
        let target = usb_target();
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
        let error = require_unique_device::<()>(Vec::new(), &usb_target())
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
        assert_eq!(usb_target().to_string(), "0416:5011, interface 0, OUT 0x01");
    }

    fn usb_target() -> UsbTarget {
        UsbTarget {
            vendor_id: 0x0416,
            product_id: 0x5011,
            serial_number: None,
            interface: 0,
            out_endpoint: 0x01,
        }
    }

    #[derive(Default)]
    struct RecordingTransport {
        request: Option<(UsbTarget, Vec<u8>)>,
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
            .expect("the system clock should be after the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "escpost-printing-{case}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("the test directory should be creatable");
        path
    }
}
