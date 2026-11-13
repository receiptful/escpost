# Direct Windows USBPRINT acceptance checks

This probe tests ESC/POS output through the printer's existing `usbprint.sys`
device interface. It does not install a driver, configure a print queue, or use
the spooler API. Run it in the **tester**, with a real USB printer attached.

Build the normal `escpost` executable in the Windows builder with the usual
static CRT settings, then copy `escpost.exe` and `usbprint.ps1` to
`C:\Users\Tester\Desktop\USBPRINT-Production`. No feature flag or backend option
is needed. The harness registers a printer using normal discovery into an
isolated configuration and exercises file input, automatic streaming stdin,
and the workbench's HTTP print endpoint.

From a normal Tester PowerShell window:

```powershell
cd "$env:USERPROFILE\Desktop\USBPRINT-Production"
# Discover the matching interface and record the driver/user facts without printing.
.\usbprint.ps1 -Executable .\escpost.exe -VendorId 0416 -ProductId 5011 -OutputDirectory .\inspect

# Print a binary file, three streaming chunks, and a workbench HTTP receipt.
.\usbprint.ps1 -Executable .\escpost.exe -VendorId 0416 -ProductId 5011 -OutputDirectory .\print-test -Print
```

The probe requires exactly one matching connected USBPRINT interface and refuses
to print as an administrator. It records its executable hash, Windows identity,
driver, device instance, configuration, spooler state, input timing, exit codes, and byte counts in
`results.json`. Confirm physical output separately: successful writes do not
prove that the printer fed paper or rendered the receipt. Keep each run in its
own output directory.

The HTTP check temporarily starts the copied executable on loopback port 19079
and stops it afterwards. It refuses an occupied port. The installed ESCPost app
and the user's printer configuration are not changed.

For automation, `run-as-tester.ps1` runs the probe through a temporary scheduled
task using Tester's limited interactive token. Run this wrapper as Controller;
Tester must already be logged in. It expects the desktop directory above:

```powershell
.\run-as-tester.ps1 -VendorId 0416 -ProductId 5011 -Run acceptance
.\run-as-tester.ps1 -VendorId 0416 -ProductId 5011 -Run acceptance -WithoutSpooler
```

The second command stops the spooler and restores its original running state in
`finally`. Both commands preserve evidence and refuse to overwrite a prior run.
The actual printing still runs as Tester. Repeat for the
second printer (`0483:5743` on our test hardware). Both can be attached together
with repeated `--usb DEVICE` options, then checked one after another without
restarting Windows. Changing the attachment set uses the tester stop/start
commands and preserves the VM disk.

The harness kills a command that has not exited within 15 seconds after EOF.
Native tests in `escpost-usbprint` separately exercise binary writes, stalled-I/O
cancellation and disconnects using named pipes. Application tests cover
ambiguous interface selection and stale endpoint configuration. Those tests do
not substitute for physical testing on additional printers and Windows versions.

Spooler queue integration and coordination with concurrently printing Windows
jobs remain deferred. Never retry a failed transfer through another backend:
it may already have printed a prefix. Historical feasibility results and normal
backend acceptance evidence are recorded in [RESULTS.md](RESULTS.md).
