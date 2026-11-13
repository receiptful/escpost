use std::cell::UnsafeCell;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::time::Duration;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Foundation::{ERROR_GEN_FAILURE, ERROR_IO_PENDING, WAIT_TIMEOUT};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, WriteFile};
use windows_sys::Win32::System::IO::{
    CancelIoEx, GetOverlappedResult, GetOverlappedResultEx, OVERLAPPED,
};
use windows_sys::Win32::System::Threading::CreateEventW;
use windows_sys::core::GUID;

const USBPRINT: GUID = GUID::from_u128(0x28d78fad_5a12_11d1_ae5b_0000f803a8c2);
const WRITE_CHUNK_BYTES: usize = 16 * 1024;

/// One live USBPRINT interface on an already selected physical USB device.
pub struct PrinterInterface {
    pub path: PathBuf,
    /// Composite function number. `None` is a driver bound to the whole device;
    /// callers must verify that its configuration has only one USB interface.
    pub number: Option<u8>,
}

/// Enumerate only this physical device and its immediate composite functions.
/// Never identify printers by friendly name or manufacture a device path.
pub fn interfaces(instance_id: &OsStr) -> io::Result<Vec<PrinterInterface>> {
    let instance = wide(instance_id)?;
    let mut root = 0;
    // SAFETY: instance is NUL terminated and root is writable for the call.
    cr(unsafe { CM_Locate_DevNodeW(&mut root, instance.as_ptr(), 0) })?;
    let mut result: Vec<_> = paths(&instance)?
        .into_iter()
        .map(|path| PrinterInterface { path, number: None })
        .collect();
    let mut child = 0;
    // SAFETY: root is a live devnode returned by Config Manager; output is valid.
    let status = unsafe { CM_Get_Child(&mut child, root, 0) };
    if status == CR_NO_SUCH_DEVNODE {
        return Ok(result);
    }
    cr(status)?;
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(child) {
            return Err(io::Error::other(
                "USB device tree changed while enumerating interfaces",
            ));
        }
        let id = device_id(child)?;
        if let Some(number) =
            super::composite_interface(&String::from_utf16_lossy(&id[..id.len() - 1]))
        {
            result.extend(paths(&id)?.into_iter().map(|path| PrinterInterface {
                path,
                number: Some(number),
            }));
        }
        let mut sibling = 0;
        // SAFETY: child is a devnode; sibling is writable for the call.
        let status = unsafe { CM_Get_Sibling(&mut sibling, child, 0) };
        if status == CR_NO_SUCH_DEVNODE {
            break;
        }
        cr(status)?;
        child = sibling;
    }
    Ok(result)
}

fn paths(instance: &[u16]) -> io::Result<Vec<PathBuf>> {
    // Size/list can race hotplug. Retry the sizing race, never a print transfer.
    for _ in 0..3 {
        let mut length = 0;
        // SAFETY: all pointers remain valid; instance has a trailing NUL.
        cr(unsafe {
            CM_Get_Device_Interface_List_SizeW(
                &mut length,
                &USBPRINT,
                instance.as_ptr(),
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        })?;
        let mut buffer = vec![0u16; length as usize];
        // SAFETY: buffer has the exact length passed to Windows.
        let status = unsafe {
            CM_Get_Device_Interface_ListW(
                &USBPRINT,
                instance.as_ptr(),
                buffer.as_mut_ptr(),
                length,
                CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
            )
        };
        if status == CR_BUFFER_SMALL {
            continue;
        }
        cr(status)?;
        return Ok(buffer
            .split(|unit| *unit == 0)
            .filter(|part| !part.is_empty())
            .map(|part| PathBuf::from(OsString::from_wide(part)))
            .collect());
    }
    Err(io::Error::other(
        "USB printer interfaces kept changing during enumeration",
    ))
}

fn device_id(node: u32) -> io::Result<Vec<u16>> {
    let mut length = 0;
    // SAFETY: length is writable; node came from Config Manager.
    cr(unsafe { CM_Get_Device_ID_Size(&mut length, node, 0) })?;
    let capacity = length
        .checked_add(1)
        .ok_or_else(|| io::Error::other("device ID too long"))?;
    let mut buffer = vec![0u16; capacity as usize];
    // SAFETY: buffer includes the required terminator and is correctly sized.
    cr(unsafe { CM_Get_Device_IDW(node, buffer.as_mut_ptr(), capacity, 0) })?;
    Ok(buffer)
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "device ID contains NUL",
        ));
    }
    value.push(0);
    Ok(value)
}

fn cr(status: CONFIGRET) -> io::Result<()> {
    if status == CR_SUCCESS {
        return Ok(());
    }
    // SAFETY: this pure conversion accepts any CONFIGRET.
    Err(io::Error::from_raw_os_error(
        unsafe { CM_MapCrToWin32Err(status, ERROR_GEN_FAILURE) } as i32,
    ))
}

/// One device handle with bounded transfers. No writes are retried after errors.
pub struct Writer {
    file: File,
    timeout_ms: u32,
    failed: bool,
}

impl Writer {
    pub fn open(path: &Path, timeout: Duration) -> io::Result<Self> {
        let timeout_ms = timeout.as_millis();
        if timeout_ms == 0 || timeout_ms >= u128::from(u32::MAX) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a finite positive write timeout is required",
            ));
        }
        let file = OpenOptions::new()
            .write(true)
            .share_mode(3)
            .custom_flags(FILE_FLAG_OVERLAPPED)
            .open(path)?;
        Ok(Self {
            file,
            timeout_ms: timeout_ms as u32,
            failed: false,
        })
    }
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "USB printer transfer already failed; refusing to replay data",
            ));
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        let result = PendingWrite::new(&self.file, &bytes[..bytes.len().min(WRITE_CHUNK_BYTES)])?
            .complete(self.timeout_ms);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        // Each write waits for driver completion. FlushFileBuffers would add
        // device-specific semantics and is not required for this unbuffered API.
        if self.failed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "USB printer transfer already failed",
            ));
        }
        Ok(())
    }
}

struct PendingWrite<'a> {
    file: &'a File,
    _event: OwnedHandle,
    // Windows updates completion fields while I/O is pending. Keep a stable
    // allocation and use only UnsafeCell::get(), never Rust references to the
    // OVERLAPPED itself, until the operation has completed or been drained.
    overlapped: Box<UnsafeCell<OVERLAPPED>>,
    bytes: Vec<u8>,
    pending: bool,
}

impl<'a> PendingWrite<'a> {
    fn new(file: &'a File, bytes: &[u8]) -> io::Result<Self> {
        // SAFETY: null security/name pointers request an unnamed manual-reset event.
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateEventW returned a new owned, non-null handle.
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        let overlapped = Box::new(UnsafeCell::new(OVERLAPPED {
            hEvent: event.as_raw_handle(),
            ..Default::default()
        }));
        Ok(Self {
            file,
            _event: event,
            overlapped,
            bytes: bytes.to_vec(),
            pending: false,
        })
    }

    fn complete(mut self, timeout_ms: u32) -> io::Result<usize> {
        let mut transferred = 0;
        // SAFETY: the owned buffer and boxed OVERLAPPED retain their addresses
        // until completion, including on timeout/unwind through Drop. The file
        // and event also outlive every pending operation. Writes are serialized.
        // UnsafeCell permits Windows to update completion fields without
        // aliasing a Rust reference to OVERLAPPED. Only Windows accesses them.
        let success = unsafe {
            WriteFile(
                self.file.as_raw_handle(),
                self.bytes.as_ptr(),
                self.bytes.len() as u32,
                &mut transferred,
                self.overlapped.get(),
            )
        };
        if success == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
                return Err(error);
            }
            self.pending = true;
            // SAFETY: these are the same live handle and OVERLAPPED used above.
            let completed = unsafe {
                GetOverlappedResultEx(
                    self.file.as_raw_handle(),
                    self.overlapped.get(),
                    &mut transferred,
                    timeout_ms,
                    0,
                )
            };
            if completed == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(WAIT_TIMEOUT as i32) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "USB printer write timed out; some bytes may already have printed",
                    ));
                }
                // Drop also cancels/drains on unexpected wait errors, so no
                // kernel operation can outlive its buffer even on failure.
                return Err(error);
            }
            self.pending = false;
        }
        if transferred == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "USB printer accepted zero bytes",
            ));
        }
        Ok(transferred as usize)
    }
}

impl Drop for PendingWrite<'_> {
    fn drop(&mut self) {
        if !self.pending {
            return;
        }
        // SAFETY: cancellation is only a request. Wait for completion before
        // releasing the buffer/OVERLAPPED/event, even if cancellation loses a
        // race to normal completion. A broken kernel driver may delay cleanup;
        // returning early and freeing the buffer would be memory-unsafe.
        unsafe {
            CancelIoEx(self.file.as_raw_handle(), self.overlapped.get());
            let mut transferred = 0;
            GetOverlappedResult(
                self.file.as_raw_handle(),
                self.overlapped.get(),
                &mut transferred,
                1,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_INBOUND;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    fn pipe(timeout: Duration) -> (Writer, File) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            r"\\.\pipe\escpost-usbprint-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let wide = wide(OsStr::new(&name)).unwrap();
        // SAFETY: valid terminated name, no optional security attributes.
        let raw = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_WAIT,
                1,
                512,
                512,
                0,
                null(),
            )
        };
        assert_ne!(raw, INVALID_HANDLE_VALUE);
        // SAFETY: successful creation gives this test ownership of the handle.
        let reader = unsafe { File::from_raw_handle(raw) };
        let writer = Writer::open(Path::new(&name), timeout).unwrap();
        // SAFETY: server handle is live, synchronous connection needs no OVERLAPPED.
        if unsafe { ConnectNamedPipe(reader.as_raw_handle(), null_mut()) } == 0 {
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(ERROR_PIPE_CONNECTED as i32)
            );
        }
        (writer, reader)
    }

    #[test]
    fn writes_preserve_multiple_binary_chunks_and_close_cleanly() {
        let (mut writer, mut reader) = pipe(Duration::from_secs(2));
        let receive = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = reader.read_to_end(&mut bytes);
            bytes
        });
        let bytes: Vec<u8> = (0..256).cycle().take(70_000).map(|n| n as u8).collect();
        writer.write_all(&bytes).unwrap();
        writer.write_all(b"last\0\xff").unwrap();
        writer.flush().unwrap();
        drop(writer);
        let mut expected = bytes;
        expected.extend_from_slice(b"last\0\xff");
        assert_eq!(receive.join().unwrap(), expected);
    }

    #[test]
    fn stalled_write_is_cancelled_and_writer_cannot_replay() {
        let (mut writer, _reader) = pipe(Duration::from_millis(50));
        let start = Instant::now();
        let error = writer.write_all(&vec![42; 128 * 1024]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(
            writer.write(b"retry").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }

    #[test]
    fn disconnect_reports_failure_instead_of_reopening_the_device() {
        let (mut writer, reader) = pipe(Duration::from_secs(1));
        drop(reader);
        assert!(writer.write_all(b"receipt").is_err());
        assert_eq!(
            writer.write(b"retry").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
