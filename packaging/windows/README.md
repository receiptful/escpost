# Windows packaging

Build the Windows x64 executable, portable ZIP, and per-user Inno Setup installer
from Linux. The frontend builds in the root Compose stack. Rust tests, the release
binary, and the installer build in a dedicated Windows 11 VM managed by Compose.
End users need none of these build tools, Docker, or Python.

## Commands

```bash
# A release: clean checkout, workspace version X.Y.Z, tag vX.Y.Z at HEAD.
just packaging windows build

# A local development artifact; includes uncommitted, non-ignored source changes.
just packaging windows build --dev

# Use a local Windows installation ISO (remembered for subsequent builds).
just packaging windows build --dev --windows-iso /path/to/windows.iso

# Choose a different artifact directory. Existing artifacts are never overwritten.
just packaging windows build --dev --output /tmp/escpost-windows

just packaging windows status
just packaging windows stop
just --list packaging windows
```

The executable entry point is `packaging/windows/build`; Just forwards arguments
without interpreting paths. It never tags, commits, uploads, signs, or publishes.
The root native-development commands retain their existing behavior.

## Requirements

- Linux x86-64 with Docker Compose and accessible `/dev/kvm` and `/dev/net/tun`
  on the Docker host. The script tests device access through Docker.
- Bash, Git, OpenSSH client tools, OpenSSL, GNU tar/coreutils, and `flock` on the
  host. Rust, Python, Bun, and MSVC are not required on the host.
- Approximately 8 GB available RAM and ample free disk space. The VM's default
  virtual disk is 96 GB and grows as used. Initial Windows installation and
  tool downloads can take tens of minutes; subsequent builds reuse caches.
- Internet access for Windows installation, pinned tool downloads, and Cargo
  dependencies. Use an appropriately licensed Windows installation image.

The guest tooling is pinned in `builder/toolchain.json`: Rust, Rustup, a
[fixed-version MSVC bootstrapper](https://learn.microsoft.com/en-us/visualstudio/releases/2022/release-history),
the Windows SDK component, and Inno Setup. Downloaded installers are checked
against recorded SHA-256 hashes. The VM container is pinned by digest in Compose.
Windows servicing updates are not frozen; record the OS build during release
validation. Changing the MSVC pin requires a fresh builder VM, rather than
silently keeping an incompatible cached installation.

## State and resources

Persistent state is git-ignored and stays inside this checkout. The builder and
tester share installation media, but have independent Windows disks:

```text
packaging/windows/
├── .state/media/windows.iso  # Shared local installation media
├── builder/.state/
│   ├── storage/             # Builder disk, guest toolchains, Cargo caches
│   ├── oem/                 # First-boot SSH setup and public key
│   ├── ssh/                 # Private key and dedicated known_hosts
│   ├── jobs/                # Staged sources, metadata, hashes, build logs
│   ├── compose.env          # Generated password and initial ports
│   ├── windows-iso.txt      # Remembered installation ISO path
│   └── build.lock           # Serializes builder operations
└── tester/.state/
    ├── baseline/            # Sealed, stopped clean Windows storage + metadata
    ├── storage/             # Active acceptance/manual VM disk
    ├── oem/                 # First-boot controller and standard-user setup
    ├── ssh/                 # Separate controller key and known_hosts
    ├── input/               # Installer snapshots, identified by SHA-256
    ├── runs/                # JSON results, logs, screenshots per run
    ├── compose.env          # Generated password and initial ports
    └── build.lock           # Serializes tester operations
```

The guest stores toolchains and compilation caches in `C:\EscpostBuilder`.
Each build clears compiled workspace crates so Cargo cannot reuse a test binary
from a previous source snapshot; third-party dependency caches are retained.
Failed jobs retain their sources and logs for diagnosis. Successful jobs are
also retained; stop the VM before reclaiming its disk/state. Do not commit,
publish, or copy `.state/` into a source archive: it includes local credentials.
Stopping the VM preserves all state. No existing Nogui VM or disk is used.

To reuse an installation ISO from another project without depending on that
project's directory, copy it to `.state/media/windows.iso`. Both machines discover
this shared location. An explicit builder `--windows-iso` path is remembered and
also serves as a fallback for the tester. The builder does not copy
externally supplied ISOs automatically. The installed VM disk is separate from
this installation media.

The Compose project name includes a hash of the checkout path. Worktrees therefore
have separate disks and caches. Ports default to SSH `2232` and desktop viewer
`8026`, bound only to `127.0.0.1`. For concurrent worktrees, choose distinct ports:

```bash
ESCPOST_WINDOWS_SSH_PORT=2242 ESCPOST_WINDOWS_VIEWER_PORT=8036 \
    just packaging windows build --dev
```

Initial port choices are saved in `.state/compose.env`. Environment variables can
override them. Resource overrides are `ESCPOST_WINDOWS_RAM` (default `8G`),
`ESCPOST_WINDOWS_CPUS` (`4`), and `ESCPOST_WINDOWS_DISK` (`96G`); use the same
overrides on subsequent starts. Shrinking an existing VM disk is unsupported.
The first boot can be watched at `http://127.0.0.1:8026/`.

SSH uses a generated key and disables password authentication. The desktop viewer
is intended for local debugging. The VM remains running after builds until stopped.

## Artifacts and installer behavior

```text
dist/windows/
├── escpost-<version>-windows-x64.zip
├── escpost-<version>-windows-x64-setup.exe
└── escpost-<version>-windows-x64-SHA256SUMS
```

Development filenames use `<version>-dev.<commit>[-dirty]`. The executable reports
the Cargo product version. The ZIP contains `escpost.exe`, quick-start instructions,
and license files. It requires no installation and makes no PATH changes.

The installer targets Windows 11 x64, installs to
`%LOCALAPPDATA%\Programs\ESCPost`, and creates an ESCPost Workbench shortcut
running `escpost.exe serve --listen --web-listen`. It keeps the console visible
and does not register startup tasks, services, firewall exceptions, or drivers.
The app's listeners default to loopback.

Setup, the Workbench Start-menu shortcut, and the Installed Apps entry use
`escpost.ico`. The Windows-first simplified mark has a transparent PNG master at
`assets/escpost-mark-simple.png`, with its generation prompt beside it. Other
ESCPost surfaces retain their existing artwork while this design is evaluated.
`make-icon.ps1` resamples the master into 16, 24, 32, 48, 64, 128, and 256 pixel
ICO frames using Windows' built-in drawing library; run it in the Windows
builder to regenerate the checked-in ICO. Inno Setup also uses this icon for
the uninstaller. The icon is installed alongside the executable and removed
on uninstall.
The installed filename is `escpost-mark.ico`; upgrading removes our old
`escpost.ico` and changes the shortcut's icon path to refresh Windows' cache.

Adding the install directory to the **user** PATH is checked by default. Upgrades
remember that choice and avoid duplicates. Uninstall removes only a PATH entry
the installer added, preserving unrelated entries and existing user configuration
in `%APPDATA%\receiptful\escpost\config`. Open a new terminal after installation.
Inno Setup handles running applications through its normal close-applications
workflow; the installer does not kill every process named `escpost.exe`.

Direct USB and RAW TCP remain part of the executable. Windows direct USB requires
WinUSB and physical testing with the target printer. The installer does not
replace printer drivers. The Windows spooler backend is deferred in
[`docs/TODO.md`](../../docs/TODO.md).

## Unsigned distribution

Signing is deferred. The browser can warn that the download is uncommon, and
SmartScreen can display **Windows protected your PC** and an unknown publisher.
Users who have verified the download source may be offered **More info → Run
anyway**. Managed policies can forbid continuation. Each new unsigned release
has to establish its own file reputation.

Windows 11 Smart App Control can block an unsigned executable that its cloud
service does not recognize as safe. It has no per-app override. The portable ZIP
does not bypass these checks. Do not instruct users to disable system-wide
security protections. Publish the unsigned status and these limitations alongside
downloads. Checksums establish file integrity, not authenticated publisher identity.

References: [SmartScreen reputation](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation),
[Smart App Control](https://support.microsoft.com/en-us/windows/security/threat-malware-protection/smart-app-control-frequently-asked-questions).

## Validation

`just packaging windows test` checks source staging, release guards, and tester
disk-restoration safeguards in the
development container. Rebuild that image with `docker compose build` after
changing its Dockerfile. Staging includes only relevant Git-managed/non-ignored
inputs plus the generated frontend bundle; it excludes configuration, VM state,
root build artifacts, and frontend dependencies. Do not edit sources while a
packaging build is in progress.

Every Windows build runs:

1. `cargo test --locked --workspace --exclude escpost-python` for the Windows
   target. Unix signal/PTY tests remain Unix-only; the separate acceptance
   tester exercises Windows Ctrl+C through the desktop.
2. A locked release build of `escpost` with static C-runtime linking and a DLL
   import check for accidental VC/Python/libusb runtime dependencies.
3. Release smoke checks from a directory without frontend sources: version,
   rendering a PNG, HTTP health, and embedded HTML/JavaScript.
4. Installer install/reinstall/uninstall checks, including checked-by-default
   PATH, duplicate prevention, opt-out persistence, pre-existing PATH ownership,
   shortcut arguments, and preservation of configuration.
5. SHA-256 verification after retrieving all artifacts. The checksums file is
   placed in the output directory last as the completed-set marker.

### Clean-machine acceptance

`tester/` provisions a second Windows 11 VM from the shared ISO. It contains
Windows, OpenSSH for host control, and a PowerShell acceptance harness. It has
no Rust, Python, Node, Inno Setup, application sources, or builder caches.
The controller uses a separate administrator account; every installer and app
check runs as the ordinary **Tester** user on its interactive desktop.
The VM uses UTC, synchronized with the host when starting a session.

```bash
# Input is an already-built installer. This command never rebuilds it.
just packaging windows verify dist/windows/escpost-<version>-windows-x64-setup.exe

# Also exercise replacement of a previous release and preservation of user data.
just packaging windows verify /path/to/candidate.exe --previous /path/to/previous.exe
```

First use installs Windows, reboots into the standard-user account, and seals a
complete stopped baseline, including firmware state. Every verification restores
a copy of that baseline. The original remains untouched. Installation media is
reused; Windows is not reinstalled for each run. Allow about 6 GB RAM and space
for both the baseline and the active disk (64 GB virtual capacity each, growing
as used). Stop the builder first on a memory-constrained host.

The checks cover the visible installer's checked PATH default, per-user install,
PATH after a real reboot, CLI version, rendering with embedded resources, exact
RAW TCP bytes, HTTP and embedded assets, Edge rendering and JavaScript errors,
the actual Start-menu shortcut and Ctrl+C, reinstall, PATH opt-out and ownership,
uninstall, and preserved configuration. Upgrade coverage is reported **not-run**
unless a distinct previous installer is supplied. A previous installer producing
the same executable is rejected as an upgrade test.

Results are kept under `tester/.state/runs/<run>/`: installer hashes, harness
hashes, host log, and `evidence/results.json`, transcripts, installer logs, PNG
output, and desktop/browser screenshots. Failure returns a nonzero status and
leaves the VM running for inspection; success stops it. An interrupted run also
retains its host log and attempts to retrieve guest evidence.

### Using the tester yourself

```bash
just packaging windows tester start --installer /path/to/ESCPost-setup.exe
just packaging windows tester status
just packaging windows tester stop

# Discard the stopped session and restore clean Windows.
just packaging windows tester reset
```

Open **http://127.0.0.1:8027/** for the local Windows desktop. Double-click
**Installers → ESCPost-setup.exe** on that desktop, follow the installer, then
launch **ESCPost Workbench** from Start. For the CLI, open a new PowerShell window
and run `escpost --version` or `escpost --help`. The workbench opens in the
Windows browser; its `localhost` is inside the VM. Close it with Ctrl+C in its
console. No RDP client is needed.

`start` resumes your current disk, and `stop` preserves it. `reset` and `verify`
refuse to run while the tester is running or paused: explicitly stop it first, because
those commands discard its current session. They retain host-side evidence.
After a failed automatic run, open the viewer directly to inspect the failure.
Save your work and close open installers before stopping Windows.

The tester uses SSH `2233` and viewer `8027`, both bound to loopback. Customize
with `ESCPOST_TESTER_SSH_PORT` and `ESCPOST_TESTER_VIEWER_PORT`; initial choices
are saved. Resource overrides are `ESCPOST_TESTER_RAM` (`6G`),
`ESCPOST_TESTER_CPUS` (`4`), and `ESCPOST_TESTER_DISK` (`64G`). Use different ports
for simultaneous worktrees. First-boot logs are in `C:\oem-bootstrap.log`.

### Remaining public-release checks

The automatic tester receives its installer locally, so it does not establish
browser-download reputation or reproduce every user's security policy. Before
public release:

- Download through a browser from the intended distribution URL, record any
  browser/SmartScreen/Smart App Control behavior, and install without elevation.
- Run acceptance with the preceding release, and also upgrade while the workbench is open;
  confirm the user is prompted to close it and configuration survives.
- Test the portable ZIP separately. Verify USB with actual hardware before
  claiming USB support verified.

Builder checks alone do not prove clean-machine or physical-printer compatibility.
Build logs live in `builder/.state/jobs/<job>/build.log`; guest job diagnostics and
installer logs live under `C:\EscpostBuilder\jobs\<job>`. First-boot SSH problems
are logged to `C:\oem-bootstrap.log`.
