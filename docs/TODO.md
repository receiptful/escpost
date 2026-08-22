# Developer-tool roadmap

## Product direction

ESCPost should become an ESC/POS developer workbench, not only a PNG
renderer.

The intended workflow is:

```text
Capture → inspect → preview → diagnose → replay → compare with hardware
```

The deterministic renderer remains the core library. Network transports, the
web interface, physical-printer access, and developer commands build around
that core without changing the submitted ESC/POS bytes.

This document describes planned work. Features listed here are not implemented
unless the current README and command coverage say otherwise. Once a feature
exists and its shape is durable, its architecture belongs in `ARCHITECTURE.md`
and the completed item can be removed from this roadmap.

`CLI.md` is the durable command-line contract. This file is the only
implementation checklist: roadmap items refer to stable CLI requirement IDs
without duplicating completion state in the contract.

## Rust CLI location

`crates/escpost/` is the Rust binary crate. Its executable remains named
`escpost`.

The crate should initially contain command parsing and the modules used only by
the developer executable:

```text
crates/escpost/
└── src/
    ├── main.rs
    ├── commands/
    │   ├── render.rs
    │   ├── inspect.rs
    │   ├── serve.rs
    │   ├── proxy.rs
    │   ├── replay.rs
    │   ├── diff.rs
    │   ├── lint.rs
    │   ├── printers.rs
    │   ├── calibrate.rs
    │   └── doctor.rs
    └── server/
        ├── raw_tcp.rs
        ├── web.rs
        ├── jobs.rs
        └── status.rs
```

This layout is illustrative rather than a requirement to create every module
up front. Add each module with the feature that needs it.

Do not create a separate server or protocol crate merely in anticipation of
reuse. Extract one when another executable or embedding API genuinely needs
the same behavior. The `escpost-render` rendering crate must remain independent of
CLI, networking, storage, and web concerns.

The Python package is now only the render binding; the Click CLI has been
removed. The root `./escpost` container wrapper invokes the Rust executable for
every command. Physical calibration reuses `render` and `print` against
`crates/escpost-profiles/calibration-job.hex` rather than a dedicated command group.

## Rust CLI foundation and render migration

These tasks implement the first vertical slice of `CLI.md`. They come before
the virtual printer because `render --web` proves the executable, renderer
integration, web server, embedded assets, and packaging model with a known
input.

### Command foundation

- [x] Create `crates/escpost` with an `escpost` binary and `clap` derive
      command model. (`CLI-G01`)
- [x] Add the global `--non-interactive` option and effective terminal-policy
      detection. (`CLI-G02`, `CLI-G03`)
- [x] Add one value-resolution layer for explicit values, metadata,
      configuration, defaults, and later interactive prompts. (`CLI-G04`)
- [x] Keep binary stdout, structured output, human status, and diagnostics on
      their documented channels. (`CLI-G05`)
- [x] Use typed command errors and nonzero failure statuses without adding a
      catch-all error dependency. (`CLI-G06`)
- [x] Handle `Ctrl+C` cleanly for long-running commands. (`CLI-G07`)
- [x] Keep the current Python hardware commands reachable during migration
      without publishing two executables named `escpost`.

### Inputs and profiles

- [x] Load raw binary files, `.hex` files, and stdin with explicit format
      overrides. (`CLI-I01`, `CLI-I02`, `CLI-I03`)
- [x] Recognize conformance-case directories and reject arbitrary directories
      as ESC/POS sources. (`CLI-I04`)
- [x] Resolve profiles from `--profile`, case metadata, or an interactive
      choice; fail clearly when unresolved in non-interactive mode.
      (`CLI-I06`)
- [ ] Add capture inputs only with the capture-store implementation.
      (`CLI-I05`)

### PNG destinations

- [x] Add `escpost render <SOURCE>`. (`CLI-R01`)
- [x] Support `-o <PNG>`, `-o -`, and `--output-dir <DIRECTORY>`.
      (`CLI-R02`)
- [x] Require exactly one sheet for a single-PNG destination unless
      `--sheet <NUMBER>` selects one. (`CLI-R03`)
- [x] Protect terminal stdout and keep piped PNG bytes exact. (`CLI-R04`)
- [x] Write deterministic sheet names and publish the manifest only after all
      PNGs succeed. (`CLI-R05`)
- [x] Overwrite explicit and conflicting generated outputs without prompting,
      render before replacing an existing file, and preserve unrelated or
      stale files. (`CLI-R08`)

### Rust web output

- [x] Add `--web`, `--browser`, `--web-listen`, and filesystem `--watch`.
      (`CLI-W01`, `CLI-W02`, `CLI-W08`)
- [x] Host the current HTML interface from the Rust executable and keep the
      initial asset embedded in the binary. (`CLI-W07`, `CLI-W10`)
- [x] Search and retain the first bindable loopback port from 9000 through
      9099 when no address is specified. (`CLI-W03`, `CLI-W04`)
- [x] Bind explicit nonzero addresses strictly, support explicit port zero,
      and make non-loopback exposure visible. (`CLI-W05`, `CLI-W06`)
- [x] Keep jobs and PNGs in memory unless a file destination was explicitly
      selected. (`CLI-W09`)
- [x] Permit file output together with web output, but reject stdout PNG output
      with a long-running web mode. (`CLI-R06`, `CLI-R07`)
- [x] Add HTTP and CLI integration coverage for `CLI-T01` through `CLI-T06`.
- [x] Verify feature parity in Docker and a real browser, then remove the
      Python `http.server` preview service and its manifest-polling workflow.
      (`CLI-T07`)
- [x] Update the root wrapper, Compose configuration, README examples, and
      architecture after the Rust path becomes authoritative.

## Rust named-printer output

This is the physical-output primitive later used by profile calibration.
`print` consumes the same immutable source loader as `render`, but it does not
render and does not require a profile.

- [x] Add `escpost print <SOURCE>` to the Rust CLI. (`CLI-P01`)
- [x] Resolve physical targets only through configured names and remove
      transport-specific options from `print`. (`CLI-P02`)
- [x] Select existing printers interactively or run the shared add workflow
      and print to the newly added target. (`CLI-P03`)
- [x] Send the decoded source bytes unchanged and report the name, target, and
      byte count without logging receipt contents. (`CLI-P04`, `CLI-P06`)
- [x] Print through configured USB or RAW TCP details and refuse unknown or
      ambiguous targets before writing data. (`CLI-P05`, `CLI-P09`)
- [x] Add typed enumeration, open, claim, endpoint, and transfer failures.
      (`CLI-P07`)
- [x] Put the physical USB boundary behind a test transport, use loopback
      network receivers, and keep ordinary automated runs incapable of
      physical printing. (`CLI-P08`)
- [x] Route `./escpost print` to Rust while leaving legacy Python
      configuration and calibration commands reachable during migration.
- [x] Verify the exact command through Docker against the connected NT-5890K
      using a small, reviewable ESC/POS smoke stream.
- [x] Verify named RAW TCP output through Docker against the connected Munbyn
      ITPP047 using the shared smoke stream.
- [x] Update README, architecture, platform, and testing documentation after
      the Rust path is verified.

## Virtual network printer

- [x] Add an `escpost serve` command.
- [x] Listen for RAW TCP print data on port 9100 by default.
- [x] Bind to `127.0.0.1` by default.
- [x] Require an explicit option to listen on LAN or public interfaces.
- [x] Select one printer profile for each listener.
- [x] Accept commands split across arbitrary TCP packet boundaries.
- [x] Render every completed job without modifying its input bytes.
- [x] Expose the captured job and its ordered PNG sheets in the web interface.
- [x] Allow separate configuration of the RAW printer port and HTTP port.
- [ ] Apply input, rendered-dot, sheet-count, connection, and retention limits.
- [x] Provide a health endpoint suitable for containers and automated tests.

Port 9100 is the common RAW/AppSocket transport used by network printers. It is
not an ESC/POS-defined job protocol and provides no authentication or
encryption. See the
[OpenPrinting network-printer documentation](https://openprinting.github.io/cups/doc/network.html).

An initial invocation could look like:

```bash
escpost serve \
  --listen 127.0.0.1:9100 \
  --profile REFERENCE \
  --web-listen 127.0.0.1:8765
```

### Job and sheet boundaries

Network connection boundaries and receipt cuts describe different things.
They must not be conflated.

- [x] Treat a TCP connection close as the default end of the active job.
- [ ] Treat Standard-mode `FF` as an explicit ESC/POS job boundary.
- [x] Treat full and partial cuts as sheet boundaries within a job. The
      renderer already splits `GS V` cuts into separate sheets, so a captured
      job's cuts appear as its ordered sheets without extra serve handling.
- [ ] Support multiple explicitly completed jobs on one persistent connection.
- [x] Offer an optional idle timeout for clients that keep a connection open
      without sending an explicit job terminator. Defaults to 20 seconds; `0`
      disables it and waits for the connection to close.
- [x] Make timeout-completed jobs visibly distinguishable from explicitly
      completed jobs. The API reports each job's completion ("closed" or
      "timeout") and the viewer flags idle-completed jobs.
- [ ] Test one-byte TCP chunks, commands split across chunks, several commands
      in one chunk, persistent connections, disconnects, and truncated jobs.

Epson describes Standard-mode `FF` as completing one series of printing
actions. See the
[Epson `FF` command reference](https://download4.epson.biz/sec_pubs/pos/reference_en/escpos/ff_in_standard.html).

## Rust web interface

- [x] Replace the Python preview server with a server hosted by the Rust CLI.
- [x] Keep ordered sheet names and responsive wrapping.
- [x] Keep original one-printer-dot-to-one-screen-pixel display as the default.
- [ ] Show live jobs newest first.
- [x] Update the page when a job arrives without requiring manual refresh.
- [ ] Show the selected profile, completion reason, connection, timestamp, and
      rendering status for each job.
- [ ] Allow rerendering a captured job with another profile.
- [x] Download the original captured binary.
- [ ] Download readable hexadecimal input, PNG sheets, command trace, events,
      and diagnostics.
- [ ] Show multiple profile renderings side by side.
- [ ] Offer exact-pixel overlay or difference views where that helps compare
      profiles or renderer versions.
- [ ] Replay a selected job to a configured physical printer.
- [ ] Export a captured job as a reproducible conformance case.
- [ ] Control simulated paper, cover, error, drawer, and online status after
      bidirectional emulation is available.
- [ ] Make persistence optional and provide an explicit retention limit.

Captured receipts can contain personal, order, and payment information. The UI
must clearly show whether jobs are held only in memory or written to disk.
Persistent capture should be opt-in unless the developer selects an explicit
local storage directory.

## Serve as a full workbench interface

The web app is the primary surface of `escpost serve`. Running `escpost serve`
should launch the web workbench and let the developer drive the whole workflow
from the browser. The RAW virtual printer becomes one opt-in capability rather
than the command's implicit purpose, which also decouples the workbench — printer
discovery, adding a printer, calibration — from any open network port.

- [ ] Make the web app `serve`'s primary role: it runs and is reachable with no
      RAW listener open. This supersedes today's behavior, where `serve` always
      binds a 9100 listener. The web app binds `--web-listen`, with `--listen`
      as a visible alias since the web is now the primary service.
- [ ] Start the RAW virtual printer only on explicit opt-in — `--virtual-listen
      [ADDR]`, whose presence enables it and whose value overrides the default
      `:9100` — instead of implicitly binding 9100.
- [ ] Enable and disable the virtual printer from the web app at runtime,
      selecting its profile, through a local control endpoint that binds and
      unbinds the RAW listener live.
- [x] Surface printer discovery — attached USB printer-class interfaces and
      configured network targets — in the web interface.
- [x] Add and configure a printer through the shared add workflow used by
      `printers add` and `print`.
- [ ] Render and print the shared calibration case, and other conformance
      cases, against a selected printer from the interface.
- [ ] Expose the remaining developer CLI commands so the browser is a complete
      alternative to the terminal for the common workflow.

## Command inspector

The inspector should explain how the byte stream changes printer state and
produces output. It should be useful even when strict rendering fails.

- [ ] Add `escpost inspect <input>`.
- [ ] Show the byte offset and raw bytes for every parsed command.
- [ ] Show the command's ESC/POS name and decoded parameters.
- [ ] Show relevant printer state before and after the command.
- [ ] Record the dot bounds painted by each printable command.
- [ ] Record paper feeds, cuts, drawer pulses, and other device events.
- [ ] Link profile-dependent behavior to the selected profile field.
- [ ] Link standard behavior to the relevant ESC/POS reference page.
- [ ] Let the web UI highlight output bounds when a command is selected.
- [ ] Let the web UI jump from a rendered element to its originating command.
- [ ] Preserve and expose the exact raw bytes after every failure.

Diagnostics must keep these cases separate:

- malformed or truncated ESC/POS;
- a valid command not yet implemented by `escpost`;
- a valid command unavailable on the selected printer profile;
- a command ignored because of its parameters or the current printer state;
- clipped or out-of-area output;
- a profile-confirmed deviation from documented ESC/POS behavior; and
- input after the last safely framed command which cannot be parsed reliably.

Strict rendering must continue to stop rather than guess after unsafe framing.
The capture and inspector layers may show the remaining opaque bytes, but must
not present speculative parsing as fact.

## Transparent physical-printer proxy

- [ ] Add an `escpost proxy` command.
- [ ] Accept the same RAW TCP input as the virtual printer.
- [ ] Forward the exact bytes to a configured USB or network printer.
- [ ] Capture and preview those bytes without delaying them unnecessarily.
- [ ] Forward physical-printer responses to the originating client.
- [ ] Never normalize, repair, or rewrite bytes in proxy mode.
- [ ] Make physical device actions explicit in the command invocation.
- [ ] Surface upstream and downstream disconnects clearly.
- [ ] Save enough metadata to replay the exact input later.
- [ ] Test backpressure, partial writes, printer disconnects, and responses
      interleaved with continued host input.

An example invocation could be:

```bash
escpost proxy \
  --listen 127.0.0.1:9100 \
  --to printer:netum-usb
```

Proxy mode provides the shortest comparison loop: an ERP sends one job, the
developer sees the PNG, and the real printer receives the identical bytes.

## Bidirectional printer emulation

Some POS applications wait for printer status or identity before continuing.
A listener which only consumes data is therefore a capture server, not yet a
complete virtual printer.

- [ ] Handle `DLE EOT` real-time status requests.
- [ ] Handle `GS a` Automatic Status Back subscriptions.
- [ ] Handle the commonly queried `GS I` printer identity forms.
- [ ] Source supported identity and status behavior from the selected profile.
- [ ] Model online, offline, cover, paper, error, feed-button, drawer, and
      cutter state as capabilities require.
- [ ] Send automatic status when an enabled state changes.
- [ ] Allow simulated state to be changed through the web UI and a local API.
- [ ] Support deterministic delayed replies for client resilience tests.
- [ ] Support deliberate disconnects and missing replies for failure tests.
- [ ] Keep real-time command recognition safe inside length-framed binary
      payloads.
- [ ] Add model-specific response forms only when a profile and evidence
      justify them.

Relevant initial references are Epson's
[`DLE EOT`](https://download4.epson.biz/sec_pubs/pos/reference_en/escpos/dle_eot.html),
[`GS a`](https://download4.epson.biz/sec_pubs/pos/reference_en/escpos/gs_la.html),
and
[`GS I`](https://download4.epson.biz/sec_pubs/pos/reference_en/escpos/gs_ci.html)
documentation.

## Developer CLI

The eventual command set and its shared interaction, input, output, and error
rules are specified in `CLI.md`.

- [ ] Accept binary files, hexadecimal text files, standard input, and captured
      job identifiers where applicable.
- [ ] Keep machine-readable JSON output separate from concise human output.
- [ ] Use nonzero exit statuses for rendering, linting, comparison, connection,
      and configuration failures.
- [ ] Ensure automation never needs to scrape the web interface.
- [x] Keep the Docker wrapper as the documented development entry point.
- [ ] Add shell completion only after command names and arguments stabilize.

### Printer discovery and diagnostics

- [x] Implement the Rust `escpost printers list` command for attached USB
      printer-class interfaces. (`CLI-M01`, `CLI-M03`)
- [x] Keep USB `printers list` read-only and label every result with its
      transport. (`CLI-M01`, `CLI-M03`)
- [x] Resolve `printers.toml` from an explicit file, `ESCPOST_CONFIG_DIR`, or
      the platform-native user configuration directory. (`CLI-M08`)
- [x] Show matched USB names and an explicit assigned or unassigned profile
      for every inventory result without creating configuration files.
      (`CLI-M09`)
- [x] Merge connected and configured records, retain unavailable printers, and
      apply the stable status-first display-name order. (`CLI-M10`)
- [x] Isolate Docker checkout configuration in a Compose-managed named volume
      instead of mounting a host installation's potentially different data.
- [x] Add safe manual RAW network registration through `printers add`, with
      terminal prompts, strict non-interactive behavior, and atomic TOML
      updates. (`CLI-M11`, `CLI-M12`, `CLI-M13`)
- [x] Register connected USB printers through the same interactive add
      workflow used by `printers add` and `print`, deriving stable descriptor
      coordinates without inferring a profile. (`CLI-M11`, `CLI-M15`)
- [x] Register a connected USB printer non-interactively by explicit
      `--vendor-id`, `--product-id`, and optional `--serial` selectors that must
      match exactly one unconfigured route, closing the last capability the
      Python `printers discover` writer still provided. (`CLI-M11`, `CLI-M15`)
- [x] List configured RAW network targets with concurrent, one-second,
      zero-byte reachability probes. (`CLI-M14`)
- [x] Add `--transport usb|network` filtering. (`CLI-M02`)
- [ ] Extend `printers list` to Bluetooth and operating-system spooler
      transports as their backends are implemented. (`CLI-M02`)
- [ ] Add versioned `--json` inventory output. (`CLI-M04`)
- [ ] Add `--status connected|unavailable` filtering when inventories become
      large enough to need it; keep sorting non-configurable.
- [x] Keep the legacy Python calibration workflow on the same resolved
      `printers.toml` until the remaining calibration commands move to Rust.
- [x] Retire the temporary Python `printers discover` configuration writer.
      Its only unique capability, a non-interactive USB config write, is now
      `printers add --vendor-id/--product-id/--serial`.
- [x] Add the Rust `escpost printers discover` command: a read-only sweep of
      directly connected IPv4 /24s (or explicit `--subnet` values up to a /16)
      that probes each host with a zero-byte connect-and-drop TCP handshake,
      skipping the scanning machine's own addresses. (`CLI-M16`, `CLI-M17`)
- [x] Register a swept host through `printers add --discover`, auto-selecting
      a single discovery result, prompting a menu for several at an
      interactive terminal, and erroring on zero or, under
      `--non-interactive`, several results. (`CLI-M18`)
- [ ] Add `printers scan` only with the first concrete active Bluetooth or
      network discovery backend. (`CLI-M05`, `CLI-M07`)
- [ ] Add `printers pair` with the first transport that needs explicit
      connection setup, delegating to the operating system where required.
      (`CLI-M06`, `CLI-M07`)
- [x] Use configured RAW network-printer targets from `print --printer`.
- [x] Add a safe direct host-and-port reachability check to `printers list`.
- [ ] Add profile-controlled status and identity probes that do not print.
- [ ] Explain USB device permissions and container group access in `doctor`.
- [ ] Avoid vendor discovery protocols (SNMP, mDNS, SLP, UDP broadcast, and
      the like) beyond the bounded TCP connect-and-drop sweep `printers
      discover` already performs, until a concrete integration needs them.
- [ ] Never send a printable probe to an unknown device without confirmation.

## Capture, replay, and regression tooling

- [ ] Give each captured job a stable local identifier.
- [ ] Preserve its immutable raw byte stream.
- [ ] Store connection and profile metadata separately from the bytes.
- [ ] Replay a capture to the virtual printer, USB, or RAW network printer.
- [ ] Replay using chosen TCP chunk sizes.
- [ ] Offer slow writes and configurable pauses.
- [ ] Disconnect at a selected byte offset.
- [ ] Delay, suppress, or alter simulated status responses.
- [ ] Export a capture into the existing conformance-case format.
- [ ] Generate expected, actual, and visual-difference PNGs for failed golden
      comparisons.
- [ ] Compare command traces, device events, diagnostics, sheet count, sheet
      dimensions, dot surfaces, and PNGs where each is relevant.
- [ ] Investigate a stream minimizer which removes bytes while preserving a
      selected parse error, rendering difference, or physical-printer symptom.

The export format should use ordinary files that are readable and reviewable
in Git. Do not introduce a database solely to store local captures. Begin with
an optional directory containing the raw bytes, small metadata, and derived
artifacts; revisit storage only when real usage proves that inadequate.

## Portability analysis

- [ ] Add `escpost lint`.
- [ ] Run one stream against one or several selected profiles.
- [ ] Report unsupported commands, code pages, symbols, and mechanisms.
- [ ] Report content outside the printable area.
- [ ] Report cutter commands for profiles without a cutter.
- [ ] Report model-dependent behavior which can materially change layout.
- [ ] Compare sheet count, dimensions, events, diagnostics, and profile
      deviations.
- [ ] Render the same capture side by side for selected profiles.
- [ ] Keep portability warnings separate from invalid-input errors.

An example invocation could be:

```bash
escpost lint receipt.bin \
  --profiles REFERENCE,NT-5890K
```

## Profile calibration workflow

- [ ] Preserve the shared physical calibration receipt.
- [ ] Render and print exactly the same version-controlled input.
- [ ] Guide developers from printer discovery to local configuration.
- [ ] Show expected output, generated output, physical evidence, and remaining
      profile TODOs together.
- [ ] Allow a captured physical-printer job to be rerendered immediately after
      a profile edit.
- [ ] Validate and explain profile fields before compiling the profile pack.
- [ ] Keep model-specific facts and evidence in the printer's profile
      directory.
- [ ] Do not infer permanent capabilities merely because one undocumented
      probe happened to print.

## Renderer performance

A receipt surface is small (about 576 dots wide by a few thousand tall) and a
full render completes in milliseconds, so none of this is urgent. Profile
before starting any item. Ordered by expected win:

- [ ] Cache rasterized glyphs. `print_character` calls fontdue's `rasterize`
      for every character occurrence, re-rasterizing identical glyphs each
      time. A cache keyed by character and pixel size removes the dominant
      per-character cost on text-heavy receipts.
- [ ] Paint runs instead of dots. Reversed cells, underlines, and scaled
      image rows call `print_dot` once per dot, paying a bounds check and a
      possible surface resize on every call. Filling whole horizontal spans
      row by row removes that overhead and gives the compiler loops it can
      auto-vectorize.
- [x] Store the surface bit-packed. `MonoSurface` stores eight MSB-first dots
      per byte in PNG's 1-bit row layout, shrinking the surface eightfold and
      reducing PNG encoding to a per-byte polarity inversion.

Explicit SIMD is deliberately absent: the workspace forbids `unsafe`,
`std::simd` is nightly-only, and the items above eliminate the hot loops SIMD
would target.

## Security and resource safety

- [ ] Default both RAW TCP and HTTP listeners to loopback.
- [ ] Warn clearly before binding RAW port 9100 to a non-loopback interface.
- [ ] Recommend a trusted LAN, VPN, or SSH tunnel for remote use.
- [ ] Do not imply that RAW port 9100 supports authentication.
- [ ] Require an explicit physical target before forwarding bytes.
- [ ] Never forward captured jobs automatically after a restart.
- [ ] Apply existing renderer resource limits to network submissions.
- [ ] Limit open connections, input rate, job duration, retained jobs, and disk
      usage.
- [ ] Treat receipt contents as potentially sensitive.
- [ ] Avoid logging entire receipt payloads unless the developer requests it.
- [ ] Make destructive device commands and simulated power commands visible in
      traces even when no physical action is taken.

## Implementation order

### Phase 1: Rust render command and web output

- [x] Complete the Rust CLI foundation and render-migration checklist above.
- [x] Verify file, directory, stdout, and web destinations end to end.
- [x] Remove the Python preview service only after the Rust web path passes its
      automated and browser checks.

### Phase 2: virtual printer

- [x] Add the RAW TCP listener and connection-close job framing.
- [x] Feed completed network jobs into the web job store shared with
      `render --web`.
- [x] Show live ordered sheets, with a waiting hint before the first job.
- [x] Offer the captured job's raw input as a download.
- [x] Add container health and transport-fragmentation tests.

### Phase 3: inspection

- [ ] Add a public command trace and structured diagnostics to the renderer.
- [ ] Add byte offsets, state changes, painted bounds, and device events.
- [ ] Expose the trace through CLI JSON and the web interface.
- [ ] Add profile switching and rerendering of captured jobs.

### Phase 4: hardware loop

- [x] Port passive USB printer listing to the Rust CLI.
- [ ] Add scanning and pairing only for transports that need them.
- [ ] Add replay to USB and RAW network printers.
- [ ] Add transparent proxy mode with response forwarding.
- [x] Retire the Click CLI. Its commands were either superseded by the Rust
      `render`/`print`/`printers` commands or reduced to `render`/`print`
      invocations against `crates/escpost-profiles/calibration-job.hex`. The richer guided
      calibration workflow remains future Phase-5 work.

### Phase 5: realistic emulation and integration testing

- [ ] Add status and printer-identity responses.
- [ ] Add controllable printer faults and timing behavior.
- [ ] Add multi-profile linting and visual comparisons.
- [ ] Add conformance-case export and stream minimization.

The first useful developer release should include the virtual RAW printer,
live PNG preview, command inspector, and transparent proxy. Those features
solve the normal integration loop without waiting for every post-v1 ESC/POS
command family.
