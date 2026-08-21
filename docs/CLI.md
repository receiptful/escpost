# Command-line interface

`escpost` is the command-line toolbox for rendering, previewing, capturing,
and printing ESC/POS jobs, including registry-only `printers list` and
USB/network discovery through `printers discover`. This document describes
the commands available in the current release. Planned commands and options
are tracked in [`TODO.md`](TODO.md). Stable requirement identifiers for the
implemented commands appear in the final section.

## Installation

Install the CLI from crates.io with Rust 1.89 or newer:

```bash
cargo install escpost
```

Run `escpost --help` or `escpost <COMMAND> --help` for the concise built-in
reference.

## Commands

```text
escpost render       Render a known ESC/POS byte stream
escpost print        Send a known byte stream to a configured printer
escpost serve        Capture RAW TCP print jobs and preview them in a browser
escpost printers     List and register printers
escpost profiles     Browse the embedded printer-profile catalog
```

## Global option

`--non-interactive` prevents ESCPost from prompting for missing values. It may
appear before or after a subcommand:

```bash
escpost --non-interactive render receipt.bin --profile REFERENCE -o receipt.png
escpost render receipt.bin --profile REFERENCE -o receipt.png --non-interactive
```

When a required value cannot be resolved without prompting, the command exits
with an error. ESCPost also avoids prompting when standard input is not a
terminal or is being used as receipt input.

## Input sources

The `render` and `print` commands accept a positional `SOURCE`:

- a raw ESC/POS file;
- a readable hexadecimal file;
- `-` for standard input; or
- an ESCPost conformance-case directory containing `case.toml` and
  `input.hex`.

Use `--format auto|binary|hex` to select the representation. In `auto` mode,
files with a `.hex` extension and recognized case directories are hexadecimal;
other files and standard input are binary.

## `escpost render`

Render one ESC/POS source into one or more PNG sheets:

```text
escpost render [OPTIONS] <SOURCE>

Options:
    --format auto|binary|hex
    --profile <PROFILE>
    -o, --output <OUTPUT>
    --output-dir <DIRECTORY>
    --sheet <NUMBER>
    --web
    --browser
    --web-listen <ADDRESS>
    --watch
    --scale <N>
    --antialias[=true|false]
```

At least one output is required. In an interactive terminal, ESCPost can
prompt for one; with `--non-interactive`, it reports an error when none is
given.

### One PNG

`-o receipt.png` writes one PNG file. `-o -` writes only PNG bytes to standard
output:

```bash
escpost render receipt.bin \
  --profile REFERENCE \
  --output receipt.png \
  --non-interactive

generate-receipt | \
  escpost render - --format binary --profile REFERENCE -o - >receipt.png
```

If a job produces several sheets, use `--sheet <NUMBER>` to select a one-based
sheet. Without a selection, single-file output fails rather than discarding
later sheets. `--sheet` requires `--output` and cannot be combined with
`--output-dir`.

### All sheets

`--output-dir <DIRECTORY>` writes every sheet and a `manifest.json` file:

```bash
escpost render receipt.hex \
  --profile REFERENCE \
  --output-dir renderings \
  --non-interactive
```

Sheets use ordered names such as `sheet-001.png` and `sheet-002.png`. The
manifest is the authoritative list for the current render. Unrelated files in
the directory are preserved.

### Browser preview and watching

`--web` starts the local viewer and prints its URL. `--browser` also opens that
URL in the default browser. `--watch` rerenders a filesystem source after it
changes and implies web mode.

```bash
escpost render receipt.hex --profile REFERENCE --web --watch
```

Use `--web-listen <IP:PORT>` to request an exact address. Omitting it selects
the first available loopback port from 9000 through 9099. Port `0` asks the
operating system to choose a free port. Binding to a non-loopback address
exposes the receipt preview to the corresponding network.

The Docker wrapper cannot open a browser on the host. Use `--web` through the
wrapper and open the printed URL manually.

### Preview quality

`--scale <N>` accepts `1`, `2`, or `3` and renders each printer dot at `N × N`
preview pixels. The default is `1` for `render`. `--antialias` enables grayscale
glyph edges for display; it does not represent additional dots produced by a
physical printer.

## `escpost print`

Send the source bytes unchanged to a configured printer:

```text
escpost print [OPTIONS] <SOURCE>

Options:
    --format auto|binary|hex
    --printer <NAME>
    --config <FILE>
```

Example:

```bash
escpost print receipt.hex --printer kitchen --non-interactive
```

`--printer` refers to a name registered in `printers.toml`. If it is omitted
at an interactive terminal, ESCPost offers the available configured printers.
In non-interactive operation, an unresolved printer is an error.

For a hexadecimal source, ESCPost decodes the text and sends the resulting
bytes. It does not insert initialization, feed, cut, or other ESC/POS commands.
USB and RAW TCP connection details come from the selected printer entry.

`--config <FILE>` selects an exact printer configuration file for this
invocation.

## `escpost printers`

`printers` separates passive inventory from active discovery and connection
setup:

```text
escpost printers [--config <FILE>] add [<NAME>]
    [--transport usb|network]
    [--host <HOST>]
    [--port <PORT>]
    [--vendor-id <ID>]
    [--product-id <ID>]
    [--serial <SERIAL>]
    [--profile <PROFILE>]
    [--discover [--subnet <CIDR>]... [--timeout <MS>]]
escpost printers [--config <FILE>] list [--transport <TRANSPORT>] [--json]
escpost printers [--config <FILE>] discover
    [--port <PORT>]
    [--subnet <CIDR>]...
    [--timeout <MS>]
escpost printers [--config <FILE>] scan [--transport <TRANSPORT>]
escpost printers [--config <FILE>] pair <CANDIDATE>
```

Commands in the `printers` family resolve `printers.toml` in this order:

```text
--config <FILE>
→ $ESCPOST_CONFIG_DIR/printers.toml
→ platform user-configuration directory
```

The platform default comes from the operating system through Rust's
`directories` crate. Linux uses
`$XDG_CONFIG_HOME/escpost/printers.toml`, falling back to
`~/.config/escpost/printers.toml`. A missing implicit file means no configured
printers. Read-only commands do not create the directory or file.

### `printers add`

`add` registers a connected USB printer or a network printer whose address is
already known:

```bash
escpost printers add

escpost printers add kitchen \
  --transport network \
  --host 10.42.0.71 \
  --port 9100 \
  --profile REFERENCE
```

At an interactive terminal, selecting `usb` reads attached USB printer-class
descriptors and offers every unconfigured interface with a bulk OUT endpoint.
The developer selects a concrete route, supplies a local name, and may assign
a profile. ESCPost stores VID/PID, an available serial number, interface, bulk
OUT endpoint, and the bulk IN endpoint only when exactly one exists. A device
with several OUT endpoints appears once per endpoint so the route is never
guessed. USB bus and address appear in the menu only; they are unstable across
reconnections and are not stored.

Already configured USB identities are omitted. When otherwise identical
connected devices expose no serial numbers, registration warns that later
printing is ambiguous while both remain connected. This is preferable to
persisting a temporary USB address or silently selecting the first device.

For a network printer, host is required. When `--port` is omitted at an
interactive terminal, ESCPost prompts for it with `9100` as the default;
pressing Enter accepts that value. An explicit `--port` skips the prompt.
Non-interactive registration silently uses `9100` when the option is omitted.
In both transports an empty optional profile answer leaves the printer
unprofiled. Sending an existing ESC/POS stream does not require a rendering
profile, and no profile—including `REFERENCE`—is inferred for an unknown
printer.

`--discover` finds the host instead of requiring an already-known `--host`:
it runs the same scan as `printers discover` and feeds the chosen result into
this same registration flow.

```bash
escpost printers add kitchen --transport network --discover
```

`--discover` and `--host` are mutually exclusive, and `--discover` is only
valid for the network transport; omitting `--transport` alongside
`--discover` implies `network`. `--subnet` and `--timeout` are valid only
together with `--discover` and behave exactly as documented under
`printers discover` below. `--port` serves both roles at once: the port
probed during the scan and the port saved for the registered printer. At an
interactive terminal, one discovered host is used automatically and several
open a selection menu. Zero discovered hosts is always an error naming the
probed port. Under `--non-interactive`, exactly one discovered host is
required: several is an error listing every discovered candidate so the
developer can retry with an explicit `--host`.

A USB printer can also be selected without a menu by naming its stable
descriptor. `--vendor-id` and `--product-id` accept decimal or `0x`-prefixed
hexadecimal and must be given together; `--serial` further narrows otherwise
identical devices. The selectors must match exactly one unconfigured route.
No match, several matching devices, or a device that still exposes several bulk
OUT endpoints is an error rather than a guess, so a scripted registration is
as deterministic as the interactive one:

```bash
escpost --non-interactive printers add counter \
  --transport usb \
  --vendor-id 0x0416 \
  --product-id 0x5011 \
  --serial B120300001 \
  --profile NT-5890K
```

`--non-interactive` disables all questions and reports the first missing
required value. Without descriptor selectors, USB registration requires a
terminal because choosing a device and endpoint is a deliberate act; ESCPost
behaves the same way when no terminal is attached, so pipelines and CI jobs
cannot wait indefinitely for input. Network registration is fully scriptable
from host and port alone:

```bash
escpost --non-interactive printers add kitchen \
  --transport network \
  --host printer.local
```

The resulting entry is ordinary, developer-editable TOML:

```toml
[kitchen]
transport = "network"
host = "printer.local"
port = 9100
```

Adding a printer:

- creates the selected configuration directory and file when needed;
- preserves existing comments, field order, and formatting;
- reports an existing name and asks for another in interactive mode;
- refuses to replace an existing name in non-interactive mode;
- validates existing configuration before changing it;
- writes a complete temporary file before atomically replacing the
  destination;
- creates a new file with mode `0600` on Unix; and
- reports the resolved configuration path.

Registration reads USB descriptors or records the supplied network endpoint,
whether that endpoint came from `--host` or from `--discover`. It does not
send bytes, infer a profile, or prove that paper can be printed. Manual
editing remains supported.

### `printers list`

`list` is the normal read-only command, and it is registry-only: it shows
exactly the printers saved in `printers.toml`, each cross-checked against
whether it is actually reachable right now. A USB or network device that is
connected but not yet registered never appears here — finding those is
`printers discover`'s job.

The default includes every supported transport. `--transport usb|network`
narrows the result without changing its shape. The command also reports the
configuration path it read on the status channel, so a developer knows where to
register or edit printers.
The human output identifies the transport and shows the connection fields
needed by the corresponding print command. A configured USB printer is
`connected` when an attached device's OS-reported identity (vendor, product,
and serial) matches its saved descriptor; interface and endpoints on that
block always come from the saved registration, not a live descriptor read, so
checking presence never opens the device and cannot fail with a permission
error. The live bus, address, and model string are shown alongside the saved
name when connected, with the manufacturer string on its own `manufacturer:`
line directly below `model:` whenever the device reports one; otherwise the
printer is `unavailable`. A configured
network target is connected when a TCP connection to its saved host and port
succeeds; refused, unresolved, and timed-out targets are unavailable.

Every result has a `profile` row regardless of transport or connection status.
It contains the configured profile identifier or `unassigned` when no profile
has been selected yet. This keeps the inventory shape predictable while
allowing unknown printers to be registered before calibration.

Connected printers appear before unavailable printers. Within each status
group, results sort case-insensitively by display name with stable
transport-specific tie-breakers. Sorting is intentionally not configurable.
An empty registry — including a `--transport` filter that matches nothing —
prints `No printers configured.` and exits successfully.

Listing does not pair devices, change configuration, send ESC/POS data, open a
USB device, or start a broad Bluetooth or network search. It opens and
immediately closes one TCP connection to each configured network target,
using a one-second timeout. These probes run concurrently and send zero
bytes. A probe that fails is confirmed before it is believed: the target is
probed a second time two seconds later, and only when that attempt fails too
is the printer reported `unavailable`. RAW TCP is frequently single-session,
so a printer busy with a job refuses one connection while being perfectly
healthy. This costs nothing when every configured printer answers, since a
successful probe is never repeated. When one does not answer, the listing
takes about two seconds longer if the connection is refused, and four seconds
in the worst case against a host that drops packets silently — one probe
timeout, the retry delay, a second probe timeout. Probes remain concurrent,
so that cost is paid once for the whole set rather than once per printer.
USB presence comes from the operating system's device metadata alone;
when no USB printer is configured, USB is not even enumerated. After the
listing, a stderr hint always points at `printers discover` for finding
connected printers not yet in the listing, regardless of how many (if any)
configured printers were shown.

### `printers discover`

`discover` is a read-only sweep for USB and network printers that are not yet
configured. It enumerates connected USB printer-class interfaces the same way
`printers list` does, and probes a TCP connection on one port across small
directly connected IPv4 networks, reporting which hosts accept it. Unlike
`list`, USB enumeration on `discover` is best-effort: a device that cannot be
opened or inspected (for example, an operating-system permission error) is
reported as a `Warning:` line on stderr and skipped, and the sweep still
reports every other USB and network printer it found, exiting successfully.
Only a failure to enumerate USB devices at all is fatal, exactly like
`list`. On Linux, when at least one of those warnings is a permission error,
stderr prints one additional line after the warnings: `Fix USB permissions
with: sudo escpost printers grant-usb-permissions` (see `printers grant-usb-permissions` below). The
same line follows any other command's fatal USB permission error too — for
example `print` sending to a USB printer, or `printers add`'s interactive or
non-interactive USB selection — since those open the device directly instead
of tolerating the failure the way `discover` does.

```bash
escpost printers discover
escpost printers discover --subnet 10.42.0.0/24 --port 9100
escpost printers discover --transport usb
```

`--transport usb|network` narrows the sweep to one connection transport;
without it, both run. `--subnet`, `--port`, and `--timeout` configure the
network sweep only, and are rejected together with `--transport usb` since
there is then no network sweep for them to configure.

Without `--subnet`, ESCPost enumerates the machine's directly connected IPv4
networks and scans each one automatically, but only when it is at most a
`/24`. A larger directly connected network is skipped rather than swept in
full, and stderr names every adapter left out before the sweep starts, one
line each:

```text
Skipped enp5s0 (10.0.0.0/16): larger than /24, scan it with --subnet 10.0.0.0/16
Skipped weird0: its netmask does not name a scannable subnet
```

Each line states the reason first, in the wording every ESCPost interface uses
for that omission. The trailing `scan it with --subnet <CIDR>` is the
terminal's own remedy, and it appears only when a subnet could be derived at
all: an adapter whose netmask is not contiguous names no CIDR subnet, so there
is nothing to pass and no advice to give, as on the second line above.

These lines belong to automatic detection, so an explicit `--subnet` never
produces them. They are printed whenever an adapter was skipped, even when
nothing is left to scan at all: a combined sweep still has USB work to do, and
the omission has to be reported either way. On a machine whose only network is
too large, the default combined sweep therefore succeeds — it enumerates USB
and explains the missing network half — while `--transport network`, which has
no other work, fails:

```text
error: no directly connected IPv4 network is small enough to scan automatically (at most /24): enp5s0 (10.0.0.0/16): larger than /24; pass --subnet <CIDR>
```

Passing one or more `--subnet <CIDR>` values scans exactly those networks
instead: it disables the automatic network enumeration and relaxes the `/24`
cap to `/16`. Naming a subnet is a deliberate act, so the bound is far more
permissive than the automatic one, but it is not unbounded — a `/16` is
already 65,534 probes and minutes of sweeping. A wider request is refused
rather than quietly narrowed:

```text
error: subnet 10.0.0.0/8 is too large to scan (at most /16)
```

`--subnet` may be repeated to scan several networks in one sweep.

No sweep ever probes this machine's own addresses. Every local IPv4 address
that falls inside a scanned subnet is excluded from the sweep — loopback
included, and whether the subnet was detected automatically or named with
`--subnet` — so a machine running `escpost serve` never discovers its own RAW
listener. Excluded addresses are left out of the announced address count too.
To confirm that a local virtual printer is listening, read `escpost serve`'s
own output rather than scanning for it.

`--port` selects the probed port and defaults to `9100`. `--timeout <MS>`
bounds each per-host connection attempt and defaults to `1000`. Probes run
concurrently and send zero bytes; a reachable port is reported as-is and is
never assumed to be a printer. Before the sweep starts, stderr prints a
`Scanning <N> network(s) on port <port> (<count> addresses):` header, whose
address count is exactly how many probes the sweep will make, followed by one
indented line per network being scanned and, only when no `--subnet` was
given, a trailing tip pointing at `--subnet` to scan a different network:

```text
Scanning 2 networks on port 9100 (507 addresses):
  - 10.42.0.0/24 (enx0)
  - 192.168.50.0/24
Tip: pass --subnet <CIDR> to scan a different network.
```

A network carries the interface name of the local adapter it belongs to
whenever this machine sits on it, for a subnet named with `--subnet` exactly
as for an automatically detected one; a subnet for a network this machine is
not on gets no label. A progress bar then follows on stderr during the network
sweep when stderr is attached to a terminal. Interrupting the sweep with
`Ctrl+C` abandons it and prints nothing: results are reported only by a run
that finishes.

USB results are listed first, then network results, numbered continuously
across both; network results are ordered by ascending IPv4 address,
regardless of the order `--subnet` was given. Each entry uses the same block
format as `printers list` so the commands cannot drift apart. A connected
USB printer matching a saved
identity heads its block with that name, `status: configured`, a `model:`
line, and a `profile:` line (falling back to `unassigned`, exactly like
`printers list`); an unmatched USB printer heads its block with its product
string alone (falling back to a generic `USB printer` label when the device
reports none) and `status: new`, omitting the `model:` and `profile:` lines.
Either way, a `manufacturer:` line follows directly below where `model:`
would be, whenever the device reports a manufacturer string, regardless of
`status`. A network result matching a saved printer's host and port
heads its block with that name, `status: configured`, and `profile:`
(falling back to `unassigned`); an unmatched host heads its block with its
bare `host:port` endpoint and `status: new`, and omits the `profile:` line
entirely. Network results reached through a directly connected network
additionally show `interface:`, and further saved names sharing the same
host and port appear as `also configured as:` lines:

```text
[1] USB Portable Printer
    status: new
    manufacturer: YICHIP3121
    transport: usb
    usb: 0416:5011; bus 3 address 57; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300001
[2] netum-usb
    status: configured
    model: USB Portable Printer
    manufacturer: YICHIP3121
    profile: NT-5890K
    transport: usb
    usb: 0416:5011; bus 3 address 60; interface 0
    endpoints: out 0x01; in 0x81
    serial: B120300002
[3] 10.42.0.5:9100
    status: new
    transport: network
    network: 10.42.0.5:9100
    interface: enx0
```

An empty combined sweep prints `No printers discovered.` and exits
successfully; finding nothing on either transport is not an error. `discover`
never writes to `printers.toml`. Use `printers add --discover` (or `add
--transport usb` for a USB printer) to register a result. When the sweep
finds at least one printer with `status: new`, stderr prints exactly one
registration hint after the listing, chosen by which transport(s) found a
new printer: a new USB printer only prints "Register a new USB printer
with" and hints at `printers add <NAME> --transport usb`; a new network
host only prints "Register a new network printer with" and hints at
`printers add <NAME> --transport network --discover` (its target command
auto-selects a single new host or opens the picker for several, so it
never depends on how many were found); finding new printers on both
transports instead prints the transport-agnostic "Register a new printer
with" and hints at the bare `printers add <NAME>`, since the interactive
wizard it launches prompts for the transport itself.

### `printers grant-usb-permissions` (Linux only)

USB printer device nodes under `/dev/bus/usb/` are root-owned by default on
most Linux distributions, so `printers discover` degrades to a permission
warning and any command that opens the device directly — `print`, and
`printers add`'s USB selection — fails outright until something grants
access. (`printers list` is unaffected: it checks USB presence from
operating-system metadata alone and never opens the device, so it cannot hit
this error; see `printers list` above.) `grant-usb-permissions` writes the udev rule
that fixes this and exists only on Linux: the subcommand is absent from
`--help` and unrecognized if typed on macOS or Windows, where no equivalent
step is needed.

```bash
escpost printers grant-usb-permissions
sudo escpost printers grant-usb-permissions
```

Without root it fails — it was asked to grant access and cannot, so this is
an error, not merely an FYI — with exit code 1 and nothing on stdout. The
error message still carries the two ways to grant the access, so the
failure is actionable rather than a bare "requires root":

```text
error: granting USB printer access requires root

Let escpost apply it:
  sudo escpost printers grant-usb-permissions

Or run the commands yourself:
  sudo tee /etc/udev/rules.d/70-escpost-usb-printers.rules <<'EOF'
# Grant locally logged-in users access to USB printer-class devices (escpost).
SUBSYSTEM=="usb", ENV{ID_USB_INTERFACES}=="*:0701*:*", TAG+="uaccess"
EOF
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=usb
```

The second option is the exact bare-metal equivalent of the first, for
anyone who would rather not run this binary as root at all: pasted as shown
into a root shell, it applies the identical rule. The heredoc uses a quoted
`'EOF'` marker so nothing in the rule is shell-expanded, and its body lines
are intentionally flush left rather than indented like the surrounding
commands — any leading whitespace there would become part of the rule file
`tee` writes.

With root and an interactive terminal (no `--non-interactive`, and both
stdin and stderr attached to a terminal — the same `can_prompt` check
`printers add` uses), it first shows the exact rule path, content, and
`udevadm` commands it is about to apply, then asks `Write the rule and
reload udev?` with a default answer of yes. Declining prints
`Nothing changed.` and exits successfully without touching the system; only
confirming proceeds to apply the change below. With `--non-interactive`, or
without a terminal, it applies immediately without asking — the scripted
provisioning path, safe to skip the prompt for since its own default answer
is yes.

Applying writes `/etc/udev/rules.d/70-escpost-usb-printers.rules`:

```text
# Grant locally logged-in users access to USB printer-class devices (escpost).
SUBSYSTEM=="usb", ENV{ID_USB_INTERFACES}=="*:0701*:*", TAG+="uaccess"
```

The match is class-wide (USB interface class `07`, subclass `01` is the USB
printer class) rather than tied to any specific vendor or product, so it
covers any USB printer, not only ones escpost has a profile for.
`TAG+="uaccess"` grants access to whichever user holds the active local
session (the same mechanism systemd-logind already uses for input and audio
devices) instead of `MODE="0666"`, which would open the device to every
local user and process. After writing the rule, it runs `udevadm control
--reload` and `udevadm trigger --subsystem-match=usb` so the change takes
effect immediately, then prints a reminder to replug the printer and rerun
`printers discover`. Running it again is safe: an identical existing rule is
left in place (and udev is still reloaded); a rule that exists with
different content is left untouched and reported as an error showing both
versions, since it may have been hand-edited.

Either outcome that leaves the rule in place — a fresh write or an
already-current rerun — also prints how to undo it later:

```text
Undo this grant later with:
  sudo rm /etc/udev/rules.d/70-escpost-usb-printers.rules
  sudo udevadm control --reload
  sudo udevadm trigger --subsystem-match=usb
Then unplug and replug the printer to be certain access is fully revoked.
```

The trailing replug reminder is not filler: `uaccess` grants access through
a logind ACL applied when the device is plugged in, and removing the rule
does not retroactively strip that ACL from a device that is already
plugged in — only a fresh plug, which logind re-evaluates against the
now-gone rule, actually revokes it. This block is not printed when the
prompt is declined or when the rule diverges and is refused, since neither
of those actually grants anything.

## `escpost profiles`

Browse the embedded catalog of printer profiles. These commands do not access
physical printers or modify `printers.toml`.

### `profiles list`

```text
escpost profiles list
    [--vendor <NAME>]
    [--source calibrated|synthesized|virtual]
    [--search <TEXT>]
    [--json]
```

Filters compose with AND:

- `--vendor` matches a case-insensitive vendor substring;
- `--source` selects calibration provenance; and
- `--search` matches a case-insensitive substring of the profile id, vendor,
  or model.

Without `--json`, the command prints a compact table. `--json` prints the full
filtered catalog as a JSON array.

### `profiles get`

Get the complete details of one profile:

```bash
escpost profiles get NT-5890K
escpost profiles get REFERENCE --json
```

An unknown profile id is an error. `--json` prints one JSON object instead of
the human-readable detail view.

### `profiles find`

Interactively search the catalog and print the selected profile id:

```bash
escpost profiles find
```

The command requires an interactive terminal and is unavailable with
`--non-interactive`. For scripts, use `profiles list --search <TEXT>`.

## `escpost serve`

Run a virtual RAW TCP printer and preview captured jobs in the web viewer:

```text
escpost serve [OPTIONS]

Options:
    --profile <PROFILE>
    --listen <ADDRESS>
    --web-listen <ADDRESS>
    --idle-timeout <SECONDS>
    --scale <N>
    --antialias[=true|false]
    --no-open
```

Example:

```bash
escpost serve \
  --listen 127.0.0.1:9100 \
  --profile REFERENCE \
  --web-listen 127.0.0.1:9000
```

The profile defaults to `REFERENCE`. Without explicit addresses, the RAW TCP
listener selects the first free loopback port from 9100 through 9109 and the
web viewer selects one from 9000 through 9099.

A job completes when its client connection closes or after the configured
idle period. `--idle-timeout` defaults to 20 seconds; `0` disables idle
completion. The current viewer displays the most recently completed job.

The viewer opens automatically when the environment permits it. `--no-open`
(also accepted as `--no-browser`) disables that behavior. Auto-opening is also
skipped with `--non-interactive`, without a terminal, under CI, or when
`BROWSER=none`.

`--scale` accepts `1`, `2`, or `3` and defaults to `3` for the browser preview.
Antialiasing is enabled by default; pass `--antialias=false` for faithful
one-bit printer dots.

RAW TCP port 9100 has no authentication or encryption. Binding either listener
to a non-loopback address can expose receipt data and should be deliberate.

## Errors and output

Invalid invocations, missing required values, decoding failures, rendering
failures, connection errors, and transfer errors return a nonzero exit status.
Human diagnostics go to standard error when standard output carries PNG or
JSON data.

Cancellation with `Ctrl+C` shuts down long-running web and virtual-printer
processes.
