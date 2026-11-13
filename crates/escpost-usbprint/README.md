# escpost-usbprint

Safe Windows USB printer device access for ESCPost. This crate enumerates live
`GUID_DEVINTERFACE_USBPRINT` interfaces within one selected device's tree and
provides a `Write` implementation using overlapped Windows I/O.

The caller is responsible for selecting a unique physical device and validating
the configured USB interface and bulk OUT endpoint. No driver is installed,
no print queue is created, and no spooler API is used.

Writes are limited to 16 KiB transfers with a caller-supplied deadline. After a
transfer failure the writer refuses further writes; a receipt prefix may already have
printed. Timeout cleanup requests cancellation and waits for completion before
freeing memory. A malfunctioning kernel driver can delay that cleanup, because
Windows does not guarantee that every operation can be cancelled immediately.

All native calls are confined to a private module with documented pointer and
handle lifetimes. The surrounding application keeps `unsafe_code = "forbid"`.
Native Windows tests use named pipes to verify bytes, disconnects, timeout
cancellation, and failed-writer behavior without needing printer hardware.

This is a separate crate to enforce that safety boundary: a module inside the
application could not permit native FFI without relaxing the application's
`forbid` policy. Only this crate's private native module allows unsafe code;
callers use safe enumeration and `Write` APIs. Printer selection, configuration,
and CLI/HTTP behavior remain in the application. The crate is compiled into
`escpost.exe`; it adds no separately distributed DLL or helper program.

Windows owns access to the completion fields while I/O is pending. The wrapper
keeps `OVERLAPPED` in a stable `Box<UnsafeCell<OVERLAPPED>>` and passes raw pointers
from `UnsafeCell::get()` for submission, waiting, cancellation, and draining.
This permits native mutation without creating Rust references to those fields.
The allocation remains alive until the operation completes, including cancellation
cleanup; interior mutability alone does not provide lifetime or synchronization
guarantees.
