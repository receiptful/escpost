//! Typed physical-printer operation for already-loaded ESC/POS bytes.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use nusb::MaybeFuture;
use nusb::transfer::{Bulk, Out};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::application::{self, ApplicationError};
use crate::configuration::{self, ConfiguredPrinter};

pub(crate) mod cli;

const USB_WRITE_BUFFER_BYTES: usize = 16 * 1024;
const USB_TRANSFER_TIMEOUT: Duration = Duration::from_secs(10);
const NETWORK_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const NETWORK_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolve one named configured printer before any source or transport I/O.
pub(crate) struct ResolveRequest {
    pub(crate) printer_name: String,
    pub(crate) config: Option<PathBuf>,
}

/// A configured printer resolved to owned connection facts before source I/O.
pub(crate) struct ResolvedPrinter {
    printer_name: String,
    target: Target,
}

/// Send already-loaded ESC/POS wire bytes to an already-resolved printer.
pub(crate) struct Request {
    pub(crate) bytes: Vec<u8>,
    pub(crate) printer: ResolvedPrinter,
}

/// Facts about the target selected from printer configuration.
pub(crate) struct Response {
    pub(crate) printer_name: String,
    pub(crate) bytes_sent: usize,
    pub(crate) target: Target,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Usb(UsbTarget),
    Network(NetworkTarget),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UsbTarget {
    pub(crate) vendor_id: u16,
    pub(crate) product_id: u16,
    pub(crate) serial_number: Option<String>,
    pub(crate) interface: u8,
    pub(crate) out_endpoint: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NetworkTarget {
    pub(crate) host: String,
    pub(crate) port: u16,
}

impl NetworkTarget {
    pub(crate) fn endpoint(&self) -> String {
        if self.host.contains(':') && !(self.host.starts_with('[') && self.host.ends_with(']')) {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

trait UsbTransport {
    fn send(&mut self, target: &UsbTarget, data: &[u8]) -> application::Result<()>;
}

/// Resolve a named configured printer without touching its device or source.
///
/// The returned value owns every connection fact, so callers can complete this
/// preflight before reading stdin or a potentially blocking filesystem source.
pub(crate) fn resolve_target(request: ResolveRequest) -> application::Result<ResolvedPrinter> {
    let configuration = configuration::load(request.config.as_deref())?;
    let printer = configuration
        .printer(&request.printer_name)
        .ok_or_else(|| ApplicationError::UnknownConfiguredPrinter(request.printer_name.clone()))?;
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
                return Err(ApplicationError::InvalidUsbOutEndpoint(target.out_endpoint));
            }
            Target::Usb(target)
        }
        ConfiguredPrinter::Network(printer) => Target::Network(NetworkTarget {
            host: printer.host.clone(),
            port: printer.port,
        }),
    };
    Ok(ResolvedPrinter {
        printer_name: request.printer_name,
        target,
    })
}

/// Transmit the caller's exact bytes to an already resolved target.
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
    let bytes_sent = request.bytes.len();
    let ResolvedPrinter {
        printer_name,
        target,
    } = request.printer;
    match &target {
        Target::Usb(target) => transport.send(target, &request.bytes)?,
        Target::Network(target) => send_network(target, &request.bytes).await?,
    }

    Ok(Response {
        printer_name,
        bytes_sent,
        target,
    })
}

async fn send_network(target: &NetworkTarget, data: &[u8]) -> application::Result<()> {
    let endpoint = target.endpoint();
    let mut stream = timeout(
        NETWORK_CONNECT_TIMEOUT,
        TcpStream::connect((target.host.as_str(), target.port)),
    )
    .await
    .map_err(|_| ApplicationError::ConnectNetworkPrinterTimeout(endpoint.clone()))?
    .map_err(|source| ApplicationError::ConnectNetworkPrinter {
        target: endpoint.clone(),
        source,
    })?;

    timeout(NETWORK_WRITE_TIMEOUT, async {
        stream.write_all(data).await?;
        stream.shutdown().await
    })
    .await
    .map_err(|_| ApplicationError::WriteNetworkPrinterTimeout(endpoint.clone()))?
    .map_err(|source| ApplicationError::WriteNetworkPrinter {
        target: endpoint,
        source,
    })
}

struct NusbTransport;

impl UsbTransport for NusbTransport {
    fn send(&mut self, target: &UsbTarget, data: &[u8]) -> application::Result<()> {
        let matches: Vec<_> = nusb::list_devices()
            .wait()
            .map_err(ApplicationError::EnumerateUsb)?
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
        let device =
            device_info
                .open()
                .wait()
                .map_err(|source| ApplicationError::OpenUsbDevice {
                    vendor_id: target.vendor_id,
                    product_id: target.product_id,
                    source: source.into(),
                })?;
        // On Linux this temporarily detaches a kernel driver such as usblp.
        // nusb reattaches that driver when the claimed interface is dropped.
        let interface = device
            .detach_and_claim_interface(target.interface)
            .wait()
            .map_err(|source| ApplicationError::ClaimUsbInterface {
                interface: target.interface,
                source: source.into(),
            })?;
        let endpoint = interface
            .endpoint::<Bulk, Out>(target.out_endpoint)
            .map_err(|source| ApplicationError::OpenUsbOutEndpoint {
                interface: target.interface,
                endpoint: target.out_endpoint,
                source: source.into(),
            })?;
        let mut writer = endpoint
            .writer(USB_WRITE_BUFFER_BYTES)
            .with_write_timeout(USB_TRANSFER_TIMEOUT);

        writer
            .write_all(data)
            .map_err(|source| ApplicationError::WriteUsb {
                endpoint: target.out_endpoint,
                source,
            })?;
        writer.flush().map_err(|source| ApplicationError::FlushUsb {
            endpoint: target.out_endpoint,
            source,
        })
    }
}

fn require_unique_device<T>(mut matches: Vec<T>, target: &UsbTarget) -> application::Result<T> {
    match matches.len() {
        0 => Err(ApplicationError::UsbDeviceNotFound {
            vendor_id: target.vendor_id,
            product_id: target.product_id,
        }),
        1 => Ok(matches.remove(0)),
        count => Err(ApplicationError::AmbiguousUsbDevices {
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
        NetworkTarget, Request, ResolveRequest, Target, UsbTarget, UsbTransport, print,
        print_with_transport, require_unique_device, resolve_target,
    };
    use crate::application::ApplicationError;

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

        let printer = resolve_target(ResolveRequest {
            printer_name: "counter".to_owned(),
            config: Some(configuration),
        })
        .expect("the configured target should resolve");
        let response = print_with_transport(
            Request {
                bytes: vec![0x1b, 0x40, 0x00, 0xff, 0x0a],
                printer,
            },
            &mut transport,
        )
        .await
        .expect("printing should succeed");

        assert_eq!(response.printer_name, "counter");
        assert_eq!(response.bytes_sent, 5);
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

        let printer = resolve_target(ResolveRequest {
            printer_name: "kitchen".to_owned(),
            config: Some(configuration),
        })
        .expect("the configured target should resolve");
        let response = print(Request {
            bytes: vec![0x1b, b'@', 0x00, 0xff, b'\n'],
            printer,
        })
        .await
        .expect("printing should succeed");

        assert_eq!(response.printer_name, "kitchen");
        assert_eq!(response.bytes_sent, 5);
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
            ApplicationError::AmbiguousUsbDevices {
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
            ApplicationError::UsbDeviceNotFound {
                vendor_id: 0x0416,
                product_id: 0x5011,
            }
        ));
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
        fn send(&mut self, target: &UsbTarget, data: &[u8]) -> crate::application::Result<()> {
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
