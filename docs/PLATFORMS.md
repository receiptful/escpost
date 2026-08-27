# Platform support

## Purpose

ESCPost should offer the same developer workflow on Linux, macOS, and Windows
while respecting how each operating system exposes printers.

This document tracks:

- release targets and packaging;
- transport backends available on each platform;
- operating-system permissions, drivers, and limitations;
- hardware behavior that still needs validation; and
- known platform issues and practical workarounds.

This is a living compatibility document, not a promise that every planned
backend is already implemented. Current implementation status remains visible
in [`README.md`](../README.md) and `TODO.md`.

Printer-model behavior does not belong here. Store firmware quirks and physical
evidence under `crates/escpost-profiles/profiles/<profile-id>/`. Store ESC/POS command coverage in
`COMMAND_COVERAGE.md`.

## Status terms

- **Verified** means ESCPost has automated or physical evidence on that
  platform.
- **Expected** means the underlying Rust or operating-system facility supports
  the platform, but ESCPost has not verified it yet.
- **Planned** means the backend or release artifact is part of the design but
  has not been implemented.
- **Limited** means the backend works for a narrower workflow and the
  limitation is documented.
- **Unsupported** means ESCPost deliberately does not provide that path.

An upstream library describing a platform as supported is not enough for
ESCPost to mark it verified.

## Current evidence

As of 2026-07-28, development and physical-printer calibration have been
performed on Linux x86-64:

- the Rust renderer and profile compiler pass their complete automated suites;
- the Rust `render` CLI and embedded web app pass their CLI and HTTP
  integration suites in Docker and have been checked in a headless Chromium
  browser;
- Rust `printers list` enumerates the connected NT-5890K printer through
  `nusb` without claiming its interface;
- the Python render binding passes its automated suite;
- Docker Compose exposes the Linux host's `/dev/bus/usb` tree to the CLI; and
- named Rust USB output through `print` has been exercised with the connected
  NT-5890K printer; and
- named Rust RAW TCP output has been exercised through the interactive Docker
  workflow and physically confirmed on paper with a connected Munbyn ITPP047.

macOS and Windows builds have not yet been verified by ESCPost. The virtual IP
printer and native Windows spooler are planned work. The Rust CLI, embedded
web server, and direct-USB backend are implemented but still need native
macOS/Windows verification.

## Release artifacts

ESCPost should ship one self-contained executable for each supported operating
system and architecture. One compiled file cannot run unchanged across all
platforms.

Initial release targets:

| Operating system | Architecture | Distribution |
|---|---|---|
| macOS | ARM64 | Homebrew bottle and release archive |
| macOS | x86-64 | Homebrew bottle and release archive |
| Linux | x86-64 | Homebrew bottle and release archive |
| Linux | ARM64 | Release archive after native CI coverage |
| Windows | x86-64 | Release archive; package-manager support later |
| Windows | ARM64 | Consider after the x86-64 backend is verified |

Homebrew chooses a bottle matching the operating system and architecture.
Release CI must build and test each artifact on its native platform instead of
assuming that successful cross-compilation proves runtime behavior.

The native executable should embed:

- the renderer;
- the canonical printer-profile pack;
- the deterministic font;
- calibration inputs; and
- the preview application's HTML, CSS, and JavaScript.

Configuration, captured jobs, PNG output, and physical evidence remain normal
runtime files. The Python binding can continue as a separate optional package;
Homebrew users should not need Python.

## User configuration

The native CLI uses Rust's `directories` crate so an installed executable
follows each platform's user-configuration convention:

| Platform | Default `printers.toml` location |
|---|---|
| Linux | `$XDG_CONFIG_HOME/escpost/printers.toml`, or `~/.config/escpost/printers.toml` |
| macOS | `~/Library/Application Support/io.receiptful.escpost/printers.toml` |
| Windows | `%APPDATA%\\receiptful\\escpost\\config\\printers.toml` |

`ESCPOST_CONFIG_DIR` replaces the directory on every platform. An explicit
`printers --config <FILE>` replaces the complete path and has highest
precedence. Read-only commands accept a missing implicit file without creating
anything.

Docker development does not mount the host's installed ESCPost configuration.
Compose stores configuration in a project-scoped named volume mounted at
`/home/developer/.config/escpost`. The volume persists across one-off commands
and `docker compose down`; `docker compose down --volumes` removes it.
Commands report the factual container path.

## Transport strategy

ESCPost should keep transports behind one internal interface. Rendering must
not depend on the selected transport.

```text
Printer transport
├── RAW TCP
├── Direct USB
├── Windows RAW spooler
├── Serial
└── File
```

### RAW TCP

RAW TCP, commonly exposed on port 9100, is the most portable physical-printer
transport:

- it uses ordinary sockets on every target platform;
- it supports exact byte forwarding;
- it can support bidirectional ESC/POS responses when the printer exposes
  them; and
- it avoids USB driver and container-pass-through problems.

RAW TCP provides no authentication or encryption. ESCPost must bind virtual
printer and preview listeners to loopback by default and require an explicit
choice for remote access.

### Direct USB

Direct USB is necessary for discovery, endpoint access, and bidirectional
testing of USB-only printers.

The Rust `print` command uses `nusb` for direct bulk transfers:

```text
ESCPost Rust CLI → nusb → operating system USB API
```

All USB access — listing, registration, and printing — uses Rust and `nusb`.
The Rust choice avoids requiring a separately installed libusb runtime. It was
selected after considering:

- bulk endpoint discovery and transfers;
- interface claim, detach, and reattach behavior;
- Windows driver compatibility;
- hot-plug support;
- native-library and static-link licensing;
- cross-compilation; and
- real ESC/POS printer evidence.

A cross-platform USB crate does not remove platform-specific permissions or
driver requirements.

### Operating-system spoolers

A spooler backend submits bytes to a printer already configured by the
operating system.

Advantages:

- developers can keep the normal installed printer driver;
- direct USB permissions are not required; and
- Windows users do not need to replace the driver with WinUSB.

Limitations:

- the queue must accept a genuinely RAW job without transforming bytes;
- access to ESC/POS responses is commonly unavailable;
- status may describe the spooler rather than immediate printer state; and
- the spooler may report a job accepted before the printer has printed it.

The first planned native spooler backend is Windows RAW printing. Linux and
macOS CUPS integration should be added only when it solves a concrete workflow;
RAW TCP and direct USB remain the initial Unix transports.

### Serial and Bluetooth

USB-to-serial adapters belong to the serial backend, not the direct USB
printer backend.

- Linux commonly exposes serial devices under `/dev`.
- macOS commonly exposes them under `/dev/cu.*`.
- Windows exposes them as `COM` ports.

Classic Bluetooth printers may appear as an RFCOMM serial port and can then use
the serial backend. Bluetooth Low Energy is a different transport with
service- and model-specific framing; it must not be presented as generic
ESC/POS serial support.

## Linux

### Direct USB

Current status: **Verified through Rust/nusb on Linux x86-64.**

Known caveats:

- The user or container must have permission to open the USB device.
- A udev rule or membership in the device's owning group is preferable to
  running ESCPost as root.
- The `usblp` kernel driver may already own the printer interface.
- The Rust direct-USB backend detaches that driver while claiming the configured
  interface and asks the kernel to reattach it when the claim is dropped.
- Only one process should own a direct interface unless the driver and backend
  explicitly support sharing.

The current Compose CLI passes `/dev/bus/usb` into the container and adds the
host printer-device group. `escpost doctor` should eventually report the
device node, ownership, selected interface, endpoints, active kernel driver,
and whether the process can claim it.

Writing to `/dev/usb/lp*` is a separate file transport. It can be useful for
simple output, but it does not provide the same descriptor, interface, or
bidirectional access as direct USB.

### Containers

USB-enabled Docker development is currently a Linux-host workflow. Tests and
rendering remain container-portable, but physical USB access depends on the
host kernel and device permissions.

## macOS

Current status: **Expected for rendering and networking; direct USB remains
unverified.**

Known caveats:

- macOS uses neither Linux udev rules nor `/dev/bus/usb`.
- A libusb-backed build may use a Homebrew-provided library or package it with
  the application.
- Claiming an interface can conflict with an operating-system or vendor driver.
- Both Apple Silicon and Intel release artifacts need native CI.
- Docker Desktop runs containers inside a Linux virtual machine and does not
  provide Linux-style host USB pass-through by default.

The host-native ESCPost executable should be the supported macOS path for
physical USB calibration. The first macOS hardware verification should record
the OS version, architecture, USB backend, printer profile, interface, and
endpoint selection.

## Windows

Windows should expose two different printer paths because they solve different
problems.

### Windows RAW spooler

Planned default for printing to an installed Windows printer.

The equivalent python-escpos backend uses the native Win32 printing API to open
a configured printer, start a document with data type `RAW`, and write the
ESC/POS bytes unchanged.

Expected advantages:

- works with a printer already visible in Windows;
- avoids replacing its driver;
- coexists with ordinary Windows printing workflows; and
- offers the lowest-friction ESCPost installation.

Expected limitations:

- it is primarily an output path;
- ESC/POS real-time status and identity replies may not be available;
- queue acceptance does not prove physical completion; and
- driver or print-processor configuration must not transform RAW bytes.

### Windows direct USB

Implemented but unverified advanced output backend. Discovery and
bidirectional protocol testing remain planned Rust work.

Known caveats:

- The current `nusb` backend uses the Windows USB API without a libusb DLL.
- The printer interface normally needs a compatible WinUSB or libusbK driver.
- Installing WinUSB with a tool such as Zadig can replace the driver used by
  the ordinary Windows printer queue.
- WinUSB generally permits only one application to own the interface at a
  time.
- Direct USB mode must show a clear warning before asking a developer to
  change drivers.

ESCPost should default to the spooler when both paths are possible. Direct USB
should be an explicit developer choice.

### Containers

Docker Desktop runs Linux containers inside a virtual machine. Passing a
Windows USB printer into the current Linux Compose service is not a supported
hardware workflow. Use a host-native Windows executable after its direct-USB
path is verified.

## Known-issue register

Keep platform issues concise here. Detailed reproduction logs and active work
belong in the issue tracker.

| Platform/backend | Status | Caveat or missing evidence | Current direction |
|---|---|---|---|
| Linux direct USB | Verified with Rust/nusb | Device permissions and `usblp` ownership vary by host | Add diagnostics for permissions and interface ownership |
| Linux Docker USB | Verified on the development host | Requires `/dev/bus/usb` plus the correct group | Keep explicit Compose device access |
| Linux RAW TCP | Verified through Docker with physical Munbyn ITPP047 output | Generic TCP success alone cannot confirm paper output | Add optional model-specific status checks separately from printing |
| macOS direct USB | Unverified | Backend packaging and interface claiming need hardware evidence | Test the host-native Rust binary |
| macOS Docker USB | Unsupported for the physical workflow | Docker Desktop does not mirror Linux host USB access | Use the host-native binary |
| Windows spooler | Planned | RAW byte preservation and completion semantics need tests | Make this the default Windows print path |
| Windows direct USB | Implemented, unverified | May require replacing the printer driver with WinUSB | Verify named USB output through the native Rust executable |
| Windows direct USB status | Unverified | Driver and exclusive-access behavior vary | Test replies on representative printers |
| Windows Docker USB | Unsupported for the physical workflow | Printer is outside the Linux VM | Use the host-native binary |
| Classic Bluetooth serial | Planned | OS pairing and RFCOMM naming vary | Treat an exposed port as serial |
| Bluetooth Low Energy | Unsupported for now | No universal ESC/POS BLE transport exists | Add only with model-specific evidence |

## Recording new evidence

When a platform behavior is verified or a limitation is found, record:

- operating system and version;
- CPU architecture;
- ESCPost commit or release;
- transport backend;
- printer profile and connection type;
- observed behavior;
- workaround, if any; and
- a link to the test, issue, or profile evidence.

Update the matrix only after the evidence is reproducible. Do not turn one
printer firmware quirk into a platform rule.

## References

- [python-escpos printer backends](https://python-escpos.readthedocs.io/en/latest/user/printers.html)
- [python-escpos direct USB implementation](https://python-escpos.readthedocs.io/en/v3.0/_modules/escpos/printer/usb.html)
- [python-escpos Windows RAW implementation](https://python-escpos.readthedocs.io/en/v3.0/_modules/escpos/printer/win32raw.html)
- [PyUSB platform and backend support](https://github.com/pyusb/pyusb#requirements-and-platform-support)
- [libusb Windows backend and driver notes](https://github.com/libusb/libusb/wiki/Windows)
- [Rust target support](https://doc.rust-lang.org/rustc/platform-support.html)
- [`nusb` documentation](https://docs.rs/nusb/0.2.5/nusb/)
- [Homebrew bottle documentation](https://docs.brew.sh/Bottles)
