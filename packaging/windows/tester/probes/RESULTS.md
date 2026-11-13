# USBPRINT feasibility results — 2026-11-12

The experimental `escpost.exe` was built in the Windows builder and copied to
the separate Windows 11 tester. No driver was installed or replaced, and no
Windows print queue was created by this experiment. The installed ESCPost app
was not replaced.

| Printer | USB ID | Driver | Spooler running | Spooler stopped |
| --- | --- | --- | --- | --- |
| NT5890K / USB Portable Printer | `0416:5011` | `usbprint` | File and stream passed | File and stream passed |
| MUNBYN / Printer-80 | `0483:5743` | `usbprint` | File and stream passed | File and stream passed |

All four runs used the ordinary **Tester** account, with administrator membership
checked as false. Each sent a binary receipt file and three stdin chunks two
seconds apart over one connection. Every command exited successfully and reported
the expected byte count. The operator confirmed physical output from both
printers and receipt labels for both spooler states. Exact paper timing was not
independently measured.

The spooler was restored after each stopped-service run. The builder was stopped
after compilation/testing. The tester was left running with the MUNBYN attached
at host port `3-2`; the probe executable and scripts are on its desktop under
`USBPRINT-Probe`.

The tested executable SHA-256 is:

```text
251a549434e675d03cf819d15fb58ffa8d20e9fa16516fc8bac80b73269a19c8
```

Raw JSON evidence and the executable are retained locally under
`packaging/windows/tester/.state/runs/usbprint-20261112/`, with one subdirectory
per printer and spooler state. The tester has the same reports on its desktop.

Regression checks: Linux fmt, workspace Clippy with all targets/features, full
workspace tests, and focused printing tests passed. Native Windows tests with
the experiment enabled passed: 31 printing unit tests, 9 print-command tests,
and 6 print-selection tests. The native build retained the pre-existing unused
`is_permission_denied_usb_open` warning.

## What this establishes

For these two printers on this tester, standard-user ESC/POS writes work through
Windows' existing USB printer driver without WinUSB replacement or the spooler.
The spooler can remain running. The experiment does not establish coexistence
with an actively printing Windows queue, disconnect recovery, bounded I/O under
a stalled driver, compatibility with every printer, or all Windows versions.

These feasibility results motivated the integrated backend described below.
Concurrent Windows queue activity remains deferred with spooler integration;
it is not covered by the service-running check.

## Integrated backend validation — 2026-11-12

The normal executable now selects USBPRINT from the configured physical USB
device and interface, with no experimental feature or private command option.
It retains the existing USB configuration format and the WinUSB path for
devices already using that driver. Transfers use overlapped I/O with a deadline
and cancellation cleanup; errors never trigger replay through another backend.

The executable was built in the Windows builder and copied separately to
`C:\Users\Tester\Desktop\USBPRINT-Production`. Its SHA-256 is:

```text
e833e0120c4675d7af80f07bd02955d29fa46a2836a5a0efadf09dc8acc2f035
```

The physical acceptance harness registers each attached printer with the normal
`printers add --transport usb` command into an isolated configuration. It then
tests a binary file, three stdin chunks two seconds apart with automatic source
selection, and one request through the workbench's `/api/print` endpoint. The
API is tested directly; this copied debug executable does not embed the frontend.
No driver installation or queue setup is performed. The installed app is unchanged.

| Printer | Account | Driver | File | Streaming stdin | Workbench HTTP |
| --- | --- | --- | --- | --- | --- |
| NT5890K `0416:5011` | Tester, non-admin | `usbprint` | Passed, 54 bytes | Passed, 152 bytes | Passed, 54 bytes |
| MUNBYN `0483:5743` | Tester, non-admin | `usbprint` | Passed, 54 bytes | Passed, 152 bytes | Passed, 54 bytes |

Both final runs used the executable hash above with the spooler running. No
concurrent queue jobs were submitted. These checks confirm successful command
completion and driver writes; the operator's physical-paper confirmations are
from the earlier feasibility runs. Exact paper timing was not measured.

Automated regression checks passed:

- Native Windows Clippy for both affected crates, including all targets, with
  warnings denied.
- Four native USBPRINT tests: composite ID parsing, exact binary writes,
  stalled-write cancellation, and disconnect failure without reopening/replay.
- 32 Windows printing unit tests, including ambiguous function selection and
  stale interface/endpoint rejection; nine command tests and six selection and
  streaming tests.
- Linux formatting, workspace Clippy, and 710 workspace tests with two test
  threads. The initial fully parallel run hit existing web readiness deadlines
  while both VMs were busy; the lower-concurrency run passed without changing
  test deadlines.
- Read-only release-version propagation check, including the new crate.

The first harness runs stalled while PowerShell 5 serialized provider metadata
attached by `Get-Content`; reading the configuration as plain text fixed report
generation. A subsequent HTTP check used the absent debug frontend as its
readiness URL; it was corrected to `/health`. Neither issue required changing
the printer transport.

Evidence and the executable are retained locally under
`packaging/windows/tester/.state/runs/usbprint-production-20261112/`.
Passing reports are in `0416-5011-spooler-on-final/` and
`0483-5743-spooler-on-final/`. The builder is stopped; the tester remains running
with NT5890K attached at host port `3-3`, and the spooler remains running.
No installer was rebuilt in this validation.
Timeout/disconnect checks use native named pipes, not physical unplugging or
paper-out tests. Concurrent queue jobs, broad hardware/Windows coverage, and
bidirectional USBPRINT status remain outside this validation.

## Reviewed installer — 2026-11-13

Review found that the pending `OVERLAPPED` storage needed interior mutability,
not just a stable address. It now uses `Box<UnsafeCell<OVERLAPPED>>`, with raw
pointers from `.get()` throughout submission, waiting, cancellation, and draining.
Independent review confirmed the fix resolves that soundness concern.

All 656 native Windows workspace tests passed with the fix. The wrapper also
passed native Clippy and its four tests again during release recovery. Linux
formatting and workspace Clippy passed. Release rendering, embedded frontend,
system-DLL dependencies, and installer lifecycle checks passed.

The builder's explicit workspace cleanup list now includes `escpost-usbprint`.
The omission had allowed the release dependency to remain cached from the prior
snapshot even though the debug tests rebuilt it. The recovery explicitly cleaned
that release dependency and confirmed it compiled from the corrected snapshot
before packaging. One rapid reinstall check was aborted by temporary Windows
security-service file locks; the subsequent complete installer check passed
without disabling services or changing installer behavior.

Corrected artifacts are under `dist/windows/usbprint-overlapped-20261113/`.
The setup SHA-256 is:

```text
fe8c3dfc343e11825fe5ca70e7e7f4892af0a9b9444f5ac9a47675babd6bfade
```

Build and recovery logs are retained in
`packaging/windows/builder/.state/jobs/20261113T091014Z-kcB8Vi/`.
The corrected setup is staged on the tester desktop under
`Installers\ESCPost-setup-usbprint-fixed.exe` for manual installation and printing validation;
it has not been installed automatically in the tester.
