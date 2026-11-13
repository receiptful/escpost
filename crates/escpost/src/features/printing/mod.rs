//! Physical-printer operations for complete jobs and continuous raw input.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use nusb::MaybeFuture;
use nusb::transfer::{Bulk, Out};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::application::{self, ApplicationError};
use crate::configuration::{self, ConfiguredPrinter};

pub(crate) mod cli;
pub(crate) mod http;
#[cfg(windows)]
mod usbprint;

const USB_WRITE_BUFFER_BYTES: usize = 16 * 1024;
const STREAM_READ_BUFFER_BYTES: usize = 16 * 1024;
const USB_TRANSFER_TIMEOUT: Duration = Duration::from_secs(10);
const NETWORK_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const NETWORK_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolve one named configured printer before any source or transport I/O.
pub(crate) struct ResolveRequest {
    pub(crate) printer_name: String,
    pub(crate) config: Option<PathBuf>,
}

/// A configured or direct printer resolved to owned connection facts before source I/O.
pub(crate) struct ResolvedPrinter {
    printer_name: Option<String>,
    target: Target,
}

impl ResolvedPrinter {
    pub(crate) fn direct_network(target: NetworkTarget) -> Self {
        Self {
            printer_name: None,
            target: Target::Network(target),
        }
    }
}

/// Send already-loaded ESC/POS wire bytes to an already-resolved printer.
pub(crate) struct Request {
    pub(crate) bytes: Vec<u8>,
    pub(crate) printer: ResolvedPrinter,
}

/// Summary of a completed print.
pub(crate) struct Response {
    pub(crate) bytes_sent: usize,
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
        printer_name: Some(request.printer_name),
        target,
    })
}

/// Transmit the caller's exact bytes to an already resolved target.
///
/// This operation is deliberately presentation-free: callers choose names,
/// load sources, and present the destination and completion summary.
pub(crate) async fn print(request: Request) -> application::Result<Response> {
    let bytes_sent = request.bytes.len();
    let target = request.printer.target;
    match &target {
        Target::Usb(target) => {
            let target = target.clone();
            run_blocking_usb(move || UsbDeviceTransport.send(&target, &request.bytes)).await?;
        }
        Target::Network(target) => send_network(target, &request.bytes).await?,
    }
    Ok(Response { bytes_sent })
}

async fn run_blocking_usb<T, Operation>(operation: Operation) -> application::Result<T>
where
    T: Send + 'static,
    Operation: FnOnce() -> application::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(ApplicationError::UsbTransferTaskJoinFailed)?
}

/// Forward raw stdin over one connection, with bounded memory and backpressure.
/// Blocking input reads stay off the async worker. No retry can replay bytes
/// after a partial write: any input or transport failure ends the operation.
pub(crate) async fn stream_stdin(
    printer: ResolvedPrinter,
    mut input: impl Read + Send + 'static,
) -> application::Result<Response> {
    let target = printer.target;
    let bytes_sent = match &target {
        Target::Usb(target) => {
            let target = target.clone();
            run_blocking_usb(move || {
                let mut writer = open_usb_writer(&target)?;
                forward_stdin(&mut input, |bytes| {
                    write_usb_chunk(&mut writer, &target, bytes)
                })
            })
            .await?
        }
        Target::Network(target) => {
            let mut connection = connect_network(target).await?;
            let endpoint = target.endpoint();
            let runtime = tokio::runtime::Handle::current();
            tokio::task::spawn_blocking(move || -> application::Result<usize> {
                let count = forward_stdin(&mut input, |bytes| {
                    runtime.block_on(write_network_chunk(
                        &mut connection,
                        &endpoint,
                        bytes,
                        false,
                    ))
                })?;
                runtime.block_on(write_network_chunk(&mut connection, &endpoint, &[], true))?;
                Ok(count)
            })
            .await
            .map_err(ApplicationError::PrintTaskFailed)??
        }
    };
    Ok(Response { bytes_sent })
}

fn forward_stdin(
    input: &mut impl Read,
    mut send: impl FnMut(&[u8]) -> application::Result<()>,
) -> application::Result<usize> {
    let mut buffer = [0; STREAM_READ_BUFFER_BYTES];
    let mut bytes_sent = 0;
    loop {
        let count = match input.read(&mut buffer) {
            Ok(0) => return Ok(bytes_sent),
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ApplicationError::ReadStdin(error)),
        };
        send(&buffer[..count])?;
        bytes_sent += count;
    }
}

#[cfg(test)]
async fn print_with_transport(
    request: Request,
    transport: &mut impl UsbTransport,
) -> application::Result<Response> {
    let bytes_sent = request.bytes.len();
    let target = request.printer.target;
    match &target {
        Target::Usb(target) => transport.send(target, &request.bytes)?,
        Target::Network(target) => send_network(target, &request.bytes).await?,
    }

    Ok(Response { bytes_sent })
}

async fn send_network(target: &NetworkTarget, data: &[u8]) -> application::Result<()> {
    let mut stream = connect_network(target).await?;
    write_network_chunk(&mut stream, &target.endpoint(), data, true).await
}

async fn connect_network(target: &NetworkTarget) -> application::Result<TcpStream> {
    let endpoint = target.endpoint();
    timeout(
        NETWORK_CONNECT_TIMEOUT,
        TcpStream::connect((target.host.as_str(), target.port)),
    )
    .await
    .map_err(|_| ApplicationError::ConnectNetworkPrinterTimeout(endpoint.clone()))?
    .map_err(|source| ApplicationError::ConnectNetworkPrinter {
        target: endpoint.clone(),
        source,
    })
}

async fn write_network_chunk(
    stream: &mut (impl AsyncWrite + Unpin),
    endpoint: &str,
    data: &[u8],
    close: bool,
) -> application::Result<()> {
    timeout(NETWORK_WRITE_TIMEOUT, async {
        stream.write_all(data).await?;
        if close {
            stream.shutdown().await?;
        }
        Ok(())
    })
    .await
    .map_err(|_| ApplicationError::WriteNetworkPrinterTimeout(endpoint.to_owned()))?
    .map_err(|source| ApplicationError::WriteNetworkPrinter {
        target: endpoint.to_owned(),
        source,
    })
}

struct UsbDeviceTransport;

impl UsbTransport for UsbDeviceTransport {
    fn send(&mut self, target: &UsbTarget, data: &[u8]) -> application::Result<()> {
        let mut writer = open_usb_writer(target)?;
        write_usb_chunk(&mut writer, target, data)
    }
}

enum UsbWriter {
    Direct(nusb::io::EndpointWrite<Bulk>),
    #[cfg(windows)]
    Windows(escpost_usbprint::Writer),
}

impl Write for UsbWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Direct(writer) => writer.write(bytes),
            #[cfg(windows)]
            Self::Windows(writer) => writer.write(bytes),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Direct(writer) => writer.flush(),
            #[cfg(windows)]
            Self::Windows(writer) => writer.flush(),
        }
    }
}

fn open_usb_writer(target: &UsbTarget) -> application::Result<UsbWriter> {
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
    let device = device_info
        .open()
        .wait()
        .map_err(|source| ApplicationError::OpenUsbDevice {
            vendor_id: target.vendor_id,
            product_id: target.product_id,
            source: source.into(),
        })?;
    // Select the native driver before sending anything. A failed open or write
    // must never trigger a fallback that might duplicate part of a receipt.
    #[cfg(windows)]
    if let Some(writer) = usbprint::open(&device_info, &device, target)? {
        return Ok(UsbWriter::Windows(writer));
    }
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
    Ok(UsbWriter::Direct(
        endpoint
            .writer(USB_WRITE_BUFFER_BYTES)
            .with_write_timeout(USB_TRANSFER_TIMEOUT),
    ))
}

fn write_usb_chunk(
    writer: &mut impl Write,
    target: &UsbTarget,
    data: &[u8],
) -> application::Result<()> {
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
        NetworkTarget, Request, ResolveRequest, ResolvedPrinter, Target, UsbTarget, UsbTransport,
        forward_stdin, print, print_with_transport, require_unique_device, resolve_target,
        run_blocking_usb, write_usb_chunk,
    };
    use crate::application::ApplicationError;

    #[test]
    fn streaming_flushes_small_usb_chunks_before_reading_more_input() {
        use std::cell::RefCell;
        use std::io::{self, Read, Write};
        use std::rc::Rc;
        let delivered = Rc::new(RefCell::new(Vec::new()));
        struct Input {
            delivered: Rc<RefCell<Vec<u8>>>,
            next: u8,
        }
        impl Read for Input {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                assert_eq!(self.delivered.borrow().len(), usize::from(self.next));
                if self.next == 3 {
                    return Ok(0);
                }
                buffer[0] = self.next;
                self.next += 1;
                Ok(1)
            }
        }
        struct Writer {
            delivered: Rc<RefCell<Vec<u8>>>,
            pending: Vec<u8>,
        }
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.pending.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.delivered.borrow_mut().append(&mut self.pending);
                Ok(())
            }
        }
        let mut input = Input {
            delivered: Rc::clone(&delivered),
            next: 0,
        };
        let mut writer = Writer {
            delivered: Rc::clone(&delivered),
            pending: Vec::new(),
        };
        let count = forward_stdin(&mut input, |bytes| {
            write_usb_chunk(&mut writer, &usb_target(), bytes)
        })
        .unwrap();
        assert_eq!(count, 3);
        assert_eq!(*delivered.borrow(), [0, 1, 2]);
    }

    #[test]
    fn streaming_stops_at_the_first_transport_failure_without_retrying() {
        let mut input = std::io::Cursor::new(vec![0; super::STREAM_READ_BUFFER_BYTES * 2]);
        let mut calls = 0;
        let error = forward_stdin(&mut input, |_| {
            calls += 1;
            Err(ApplicationError::WriteUsb {
                endpoint: 1,
                source: std::io::Error::from(std::io::ErrorKind::BrokenPipe),
            })
        })
        .unwrap_err();
        assert!(matches!(error, ApplicationError::WriteUsb { .. }));
        assert_eq!(calls, 1);
        assert_eq!(input.position(), super::STREAM_READ_BUFFER_BYTES as u64);
    }

    #[test]
    fn streaming_retries_interrupted_reads_but_stops_on_a_real_input_error() {
        use std::io;
        struct Input(u8);
        impl Read for Input {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.0 += 1;
                match self.0 {
                    1 => Err(io::ErrorKind::Interrupted.into()),
                    2 => {
                        buffer[..3].copy_from_slice(&[0, 255, 10]);
                        Ok(3)
                    }
                    3 => Err(io::Error::other("producer failed")),
                    _ => panic!("a failed producer must not be read again"),
                }
            }
        }
        let mut delivered = Vec::new();
        let error = forward_stdin(&mut Input(0), |bytes| {
            delivered.extend_from_slice(bytes);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(delivered, [0, 255, 10]);
        assert!(
            matches!(error, ApplicationError::ReadStdin(source) if source.to_string() == "producer failed")
        );
    }

    #[test]
    fn usb_streaming_handles_short_writes_and_propagates_flush_failure() {
        use std::io::{self, Write};
        struct Writer {
            bytes: Vec<u8>,
            flushes: usize,
            fail_flush: bool,
        }
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                let count = bytes.len().min(2);
                self.bytes.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
            fn flush(&mut self) -> io::Result<()> {
                self.flushes += 1;
                if self.fail_flush {
                    Err(io::ErrorKind::BrokenPipe.into())
                } else {
                    Ok(())
                }
            }
        }
        for fail_flush in [false, true] {
            let mut writer = Writer {
                bytes: Vec::new(),
                flushes: 0,
                fail_flush,
            };
            let mut input = io::Cursor::new(vec![0, 255, 27, 64, 10]);
            let result = forward_stdin(&mut input, |bytes| {
                write_usb_chunk(&mut writer, &usb_target(), bytes)
            });
            assert_eq!(writer.bytes, [0, 255, 27, 64, 10]);
            assert_eq!(writer.flushes, 1);
            if fail_flush {
                assert!(
                    matches!(result, Err(ApplicationError::FlushUsb { source, .. }) if source.kind() == io::ErrorKind::BrokenPipe)
                );
            } else {
                assert_eq!(result.unwrap(), 5);
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn streaming_network_write_times_out_after_partial_delivery() {
        use tokio::io::AsyncReadExt;
        // A four-byte buffer with an idle reader forces the write to stall.
        // Mock time exercises the real timeout without a ten-second test.
        let (mut writer, mut reader) = tokio::io::duplex(4);
        let error =
            super::write_network_chunk(&mut writer, "printer:9100", &[0, 1, 2, 3, 4, 5], false)
                .await
                .unwrap_err();
        assert!(
            matches!(error, ApplicationError::WriteNetworkPrinterTimeout(endpoint) if endpoint == "printer:9100")
        );
        drop(writer);
        let mut delivered = Vec::new();
        reader.read_to_end(&mut delivered).await.unwrap();
        assert_eq!(delivered, [0, 1, 2, 3]);
    }

    #[tokio::test]
    async fn streaming_network_write_reports_a_disconnected_printer() {
        let (mut writer, reader) = tokio::io::duplex(4);
        drop(reader);
        let error = super::write_network_chunk(&mut writer, "printer:9100", b"receipt", false)
            .await
            .unwrap_err();
        assert!(
            matches!(error, ApplicationError::WriteNetworkPrinter { target, source }
            if target == "printer:9100" && source.kind() == std::io::ErrorKind::BrokenPipe)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn synchronous_usb_work_runs_off_the_async_worker() {
        // Defect caught: nusb enumeration, send, and flush park a Tokio worker
        // when the shared print operation is called by the HTTP server.
        let async_worker = std::thread::current().id();

        let usb_worker = run_blocking_usb(|| -> crate::application::Result<_> {
            Ok(std::thread::current().id())
        })
        .await
        .expect("the blocking USB task should complete");

        assert_ne!(usb_worker, async_worker);
    }

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
        assert_eq!(printer.printer_name, Some("counter".to_owned()));
        let response = print_with_transport(
            Request {
                bytes: vec![0x1b, 0x40, 0x00, 0xff, 0x0a],
                printer,
            },
            &mut transport,
        )
        .await
        .expect("printing should succeed");

        assert_eq!(response.bytes_sent, 5);
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
        assert_eq!(printer.printer_name, Some("kitchen".to_owned()));
        assert_eq!(
            printer.target,
            Target::Network(NetworkTarget {
                host: "127.0.0.1".to_owned(),
                port,
            })
        );
        let response = print(Request {
            bytes: vec![0x1b, b'@', 0x00, 0xff, b'\n'],
            printer,
        })
        .await
        .expect("printing should succeed");

        assert_eq!(response.bytes_sent, 5);
        assert_eq!(
            receiver.join().expect("the receiver should finish"),
            vec![0x1b, b'@', 0x00, 0xff, b'\n']
        );
        fs::remove_dir_all(directory).expect("the test directory should be removable");
    }

    #[tokio::test]
    async fn exact_bytes_reach_an_unnamed_network_target_unchanged() {
        let listener =
            TcpListener::bind(("127.0.0.1", 0)).expect("the loopback printer should bind");
        let port = listener
            .local_addr()
            .expect("the listener should have an address")
            .port();
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
        let bytes = vec![0x1b, b'@', 0x00, 0xff, b'\n'];

        let response = print(Request {
            bytes: bytes.clone(),
            printer: ResolvedPrinter::direct_network(NetworkTarget {
                host: "127.0.0.1".to_owned(),
                port,
            }),
        })
        .await
        .expect("direct network printing should succeed");

        assert_eq!(response.bytes_sent, bytes.len());
        assert_eq!(receiver.join().expect("the receiver should finish"), bytes);
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
