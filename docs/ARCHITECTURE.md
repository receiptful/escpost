# Architecture

## Purpose

`escpost-render` interprets one complete ESC/POS byte stream as an isolated print
job and returns one or more PNG receipt sheets.

The renderer works in printer dots. HTML, CSS, SVG text, host fonts, and browser
layout do not participate in positioning.

The public Rust entry point is:

```rust
pub fn render(
    data: &[u8],
    profile: &PrinterProfile,
) -> Result<RenderResult, RenderError>
```

Every render starts from the selected profile's reset defaults. State from a
physical printer before the submitted byte stream is outside the v1 input
model.

## Workspace boundaries

The Rust workspace contains four crates:

- `escpost-profiles` imports, enriches, validates, and loads printer
  profiles.
- `escpost-render` parses ESC/POS, applies printer state, rasterizes content, and
  encodes PNG.
- `escpost` provides the native `escpost` executable, PNG destinations,
  embedded local web viewer, named USB and RAW TCP output, passive printer
  inventory, and platform-native machine configuration.
- `escpost-python` exposes coarse-grained rendering functions through PyO3.

The rendering crate performs pure computation and depends on no
operating-system interface — no networking, hardware, filesystem, or clock
access. This keeps it deterministic and embeddable in any host, including
WebAssembly targets, and is a deliberate boundary documented in
`DESIGN_DECISIONS.md`.

Python calls into Rust once per job. The binding releases the Python
interpreter lock while Rust renders.

The Python package is only the render binding; it contains no CLI. The root
development wrapper routes every command to the Rust executable. Hardware
inventory and printing live in `escpost`, not the Rust rendering library.

## Native application architecture

The `escpost` crate is organized by capability. Each capability owns its
application operation and its thin CLI adapter:

```text
src/
├── application/              shared application error/result boundary
├── cli.rs                    root Clap tree
├── cli/
│   └── web.rs                shared web-viewer CLI presentation
├── lib.rs                    root command dispatch
├── features/
│   ├── printers/
│   │   ├── add/{mod,operation,cli}.rs
│   │   ├── discover/{mod,cli}.rs
│   │   ├── list/{mod,cli}.rs
│   │   ├── cli.rs              shared CLI and command dispatch
│   │   ├── cli/
│   │   │   └── grant_usb_permissions.rs
│   │   └── inventory.rs
│   ├── profiles/{mod,cli}.rs
│   ├── rendering/{mod,cli}.rs
│   ├── printing/{mod,cli}.rs
│   └── capture/{mod,cli}.rs
├── configuration.rs
├── discovery.rs
├── net.rs
├── source.rs
├── watch.rs
└── web.rs
```

A feature's `mod.rs` defines typed requests, factual responses, and operations.
Application operations and their low-level dependencies share
`application::ApplicationError`, whose variants contain factual failure context
without terminal guidance. A feature operation has no dependency on Clap,
terminal I/O, web-store updates, or wire serialization.

Its `cli.rs` converts command input into validated application values, wraps
application failures with adapter-owned recovery guidance, and presents the
structured response. Wire DTOs, serialization, terminal labels, and prose also
belong to the adapter. Operation facts retain their native types; for example,
profile provenance and barcode support remain `ProfileSource` and
`BarcodeSystem` values until the CLI maps them into human or JSON output.

```text
root CLI dispatch ─> feature::cli ─> feature operation ─> configuration, discovery, rendering, hardware
```

Capability-local `http.rs` adapters are added with the HTTP API for the
capabilities it exposes; the tree does not reserve empty HTTP adapter
placeholders. Those adapters will translate typed HTTP input into the same
feature operations:

```text
HTTP router ─> feature::http ─> feature operation
```

The root `cli.rs` contains the Clap tree, while CLI-wide adapter infrastructure
lives under `cli/`; a future `http/` module will contain shared HTTP transport
infrastructure, not mirrored copies of every operation. Low-level
operating-system modules remain at the crate root until a concrete boundary
warrants moving them. A separate application crate is justified only when a
second executable or library host needs the service layer.

Application requests contain validated execution values. Clap argument types
and HTTP request DTOs remain adapter-owned and convert explicitly into those
requests. Application responses contain facts, not terminal prose or HTTP
status codes. HTTP operations never prompt; interactive workflows belong to
the CLI or browser. Render requests carry `escpost_render::RenderScale`, whose
constructor accepts only the supported integer densities 1 through 3. Adapters
must construct it before source, configuration, listener, or device I/O.

`printers grant-usb-permissions` is the deliberate CLI-only host-command
exception. Root checks, confirmation, udev rule mutation, `udevadm` execution,
and recovery instructions live together under `features/printers/cli/`; there
is no reusable application or HTTP operation for that host-administration
workflow.

The capture operation consumes one exact, completed RAW byte vector, a
validated printer profile, and render parameters, then returns that same byte
vector with traced render facts. Its CLI adapter resolves the profile before
binding listeners and owns listener binding, idle-timeout validation, browser
policy, connection lifecycle, task spawning, terminal output, and `JobStore`
updates. It runs the synchronous capture operation through `spawn_blocking` so
a render cannot stall web responses.

### CLI output ownership and testing

The command that produces user-facing output owns its wording. Shared
application errors represent factual failure categories. CLI errors add
invocation failures, prompts, terminal and viewer failures, and command-specific
recovery text without making application modules depend on command modules.

Tests assert contracts at the boundary where those contracts exist. Unit tests
cover semantics and safety invariants such as paths, preserved bytes, required
commands, and state transitions. They do not duplicate an exact output contract
already covered at another layer or pin incidental whitespace without a reason.
Verbatim output comparisons remain appropriate when the complete output is the
behavior under test and the literal is clearer than fragmented assertions.

## Rust named-printer output

`escpost print` chooses one configured printer name, resolves and validates its
owned target facts, and only then loads the source through the same immutable
source loader as `render`. This preflight rejects a missing or invalid named
target before reading a file or blocking on stdin. The operation then hands the
decoded bytes directly to its USB or RAW TCP transport; it does not invoke the
renderer or require a printer profile.

```text
configured printer name → target preflight → load and decode source
                                                   ├── nusb bulk OUT
                                                   └── RAW TCP socket
```

Transport details live only in `printers.toml`. This keeps `print` independent
of transport-specific flags and gives calibration, inventory, and direct output
the same printer identity. Interactive output selection may call the shared
add-printer workflow; configuration is reloaded before the new name is used.

The USB implementation uses `nusb`. On Linux it detaches a kernel driver such
as `usblp` only while claiming the configured interface and reattaches it when
the interface is released. The optional configured serial number distinguishes
devices with equal VID/PID values. A buffered bulk writer waits for every
submitted transfer to complete and applies a ten-second timeout to each
blocking transfer.

The RAW TCP implementation connects directly to the configured host and port,
writes the source bytes once, and shuts down the connection without a separate
probe or protocol framing. Connection and write timeouts keep failures bounded.

Automated tests replace only the `UsbTransport` boundary and use loopback
listeners for network output. Source loading, name resolution, target
validation, and byte preservation remain real; ordinary tests cannot open or
write to configured physical hardware.

`printers list` is a metadata-only inventory of configured printers. For saved
USB entries it compares `nusb`'s operating-system device identities without
opening a device or reading active configuration descriptors; for saved
network entries it performs the bounded reachability probe. A matching saved
entry is reported as connected and an unmatched saved entry as unavailable.
When several saved aliases ambiguously match the same USB identity, the
deterministic first alias is connected and its sibling aliases are omitted
rather than double-counted. Connected USB devices with no saved identity are
also omitted entirely and belong to `printers discover`, not `list`. Connected
entries sort first, then unavailable entries, and each group sorts
case-insensitively by configured name with stable transport tie-breakers.

`printers discover` owns full USB discovery. It opens candidate printer-class
devices to inspect their active interfaces and bulk endpoints, returns tolerant
per-device warnings, and reports both configured and unconfigured devices.
`printers add` separately owns the full USB enumeration used to present and
persist a concrete interface and endpoint selection; it does not route that
selection through the metadata-only list inventory. Neither workflow claims a
printer interface, detaches a kernel driver, or sends a USB transfer.

The add operation accepts one `Connection` enum describing the desired USB or
network configuration. `Request::new` validates the name, optional profile,
and every connection field, so execution receives a valid request. Its response
returns the saved name, profile, connection, and configuration path; adapters
present or serialize those authoritative facts rather than retaining a second
copy of the request state.

The discovery CLI first converts its arguments into a valid `DiscoveryScope`
and `NetworkScan`; incompatible options and an invalid port fail before any
configuration I/O. `prepare` then loads configuration and derives scan targets,
and `execute` emits factual observer events and returns typed discovery facts.
`printers add --discover` reuses that lifecycle and retains only prompting and
selection in its CLI adapter.

All inventory commands read `printers.toml` through the same path precedence:
an explicit `--config` file, then `ESCPOST_CONFIG_DIR`, then the platform user
configuration directory resolved by Rust's `directories` crate. A missing
implicit file is an empty configuration, and read-only inventory never creates
a directory. Every mutation uses one whole-file transaction: acquire the stable
sibling `.printers.toml.lock`, read the latest complete text, derive the complete
replacement, and publish it with an atomic rename. The lock file remains in
place; the operating system releases its advisory lock when the file descriptor
closes, including after a process crash. Mutations append only the new printer
table so hand-edited comments, ordering, and formatting remain intact. Reads do
not lock because atomic replacement exposes either the old or new complete
file. Bus and address are diagnostic selection labels only because an operating
system may change them after reconnection. A serial number is stored when
available; without one, simultaneously connected devices with equal VID/PID
cannot be distinguished reliably and are reported as ambiguous.

The Docker wrapper creates and mounts `.config` at the container user's normal
ESCPost configuration path. This isolates configuration used by a checkout from
an independently installed binary while keeping Docker-specific paths out of
the Rust implementation. Commands and errors report the factual path used by
the running process; Docker does not rewrite it to a host path.

## Rust render command

`escpost` is an application boundary around the renderer. It embeds the
canonical profile pack and resolves a profile from an explicit argument,
recognized source metadata, or an interactive selection. Non-interactive
operation fails instead of silently choosing a physical printer.

The command accepts raw files, readable `.hex` files, stdin, and recognized
conformance-case directories. Output adapters consume one completed
`RenderResult`:

```text
Known ESC/POS source
        │
        ▼
Profile resolution → escpost_render::render
                           │
             ┌─────────────┼─────────────┐
             ▼             ▼             ▼
        one PNG       sheet directory   in-memory job
        or stdout     plus manifest     and web viewer
```

Single-PNG output never drops later sheets. Directory output publishes its
manifest only after all current sheets are complete. An explicit file and the
web viewer may consume the same render without parsing or rendering twice.

Rendered PNGs live in a shared in-memory job store. The viewer reports ordered
sheet names and printer-dot dimensions, uses one screen pixel per dot initially,
and offers only integer, nearest-neighbor zoom.

Watch mode polls the selected filesystem input and performs each rerender away
from the asynchronous HTTP task. A successful result atomically replaces the
visible job. A parse or render failure is reported by the page while the last
complete sheets remain available.

### Planned embedded web application

The web UI will be a Preact and TypeScript single-page application built with
Vite. Axum will own HTTP routing, JSON APIs, and static asset delivery. The
frontend will have no server-side JavaScript runtime.

Existing web-enabled commands will host the same Axum router and embedded
frontend. `serve` will add RAW job capture; `render --web`, `render --browser`,
and `render --watch` will seed or update the render job store. Every mode will
expose the same application operations. There will be no daemon, separate
backend executable, or inter-process API.

RAW capture and HTTP serving remain concurrent tasks in one process. Splitting
them into subprocesses is reserved for a measured performance or isolation
problem; the architecture does not introduce IPC speculatively.

HTTP operation paths will follow the CLI command tree without an API version
prefix:

```text
escpost printers list       GET  /printers/list
escpost printers discover   POST /printers/discover
escpost printers add        POST /printers/add
escpost profiles list       GET  /profiles/list
escpost profiles get ID     GET  /profiles/get/{id}
escpost render              POST /render
escpost print               POST /print
```

Names and parameter concepts will transfer between CLI and HTTP. HTTP will
accept typed query parameters or JSON, never argument arrays or shell strings,
and return structured data rather than captured stdout or stderr. The web
server will call the same feature operations as the CLI; it will never invoke
the `escpost` executable or call Clap handlers. HTTP-only infrastructure such as
health checks and static assets will need no CLI equivalent.

Vite will emit fixed-name HTML, JavaScript, and CSS assets. The Cargo build
script will run the pinned frontend toolchain, write its output under `OUT_DIR`,
and the `escpost` crate will embed those bytes at compile time. Release artifacts
will remain a single executable and require neither Node.js nor external web
assets at runtime. Docker images will pin Node.js and install dependencies from
the lockfile.

For development, Vite will serve the frontend with hot reload and proxy
application routes to a running escpost server. Production and test builds will
serve only the embedded assets.

Automatic listeners will continue to bind to loopback. Explicit `--web-listen`
addresses will remain supported; non-loopback bindings will retain the exposure
warning. State-changing API requests will require JSON, reject untrusted
origins, and carry a per-process same-origin capability so an unrelated browser
origin cannot mutate local printer state. Authentication, TLS, and remote
exposure will belong to an operator's reverse proxy.

## Rendering pipeline

```text
ESC/POS bytes
      │
      ▼
Sequential command parser
      │
      ▼
Profile-aware printer state
      │
      ▼
Standard-mode line composition
      │
      ▼
Monochrome dot surfaces
      │
      ▼
One-bit PNG sheets
```

The parser consumes the submitted byte slice from left to right. Commands with
binary data use their documented length fields, so payload bytes are never
searched for command prefixes.

Malformed, truncated, unknown, or unsupported input returns a `RenderError`.
V1 does not return a speculative partial preview after a parser error.

### Renderer modules

Each rendering domain owns one module in `crates/escpost-render/src/`:

```text
lib.rs            public API types and the render entry point
command.rs        sequential ESC/POS parsing and dispatch
state.rs          printer state, line composition, cuts, and limits
text.rs           code-page decoding and glyph rasterization
graphics.rs       bit-image and raster graphics painting
symbols.rs        barcode and QR placement and painting
barcode.rs        one-dimensional barcode encoders
databar.rs        GS1 DataBar encoding
qr.rs             QR matrix adapter
international.rs  ESC R character substitutions
surface/          rendering contract, monochrome raster, and tracing decorator
error.rs          renderer error types
```

`PrinterState` and its lifecycle live in `state.rs`; the text, graphics, and
symbols modules extend it with their own `impl` blocks so each painting
domain stays readable on its own. The public API is re-exported from the
crate root, so module boundaries are not visible to embedders.

The private `RenderSurface` contract keeps command interpretation independent
from raster storage. `MonoSurface` is the ordinary bitmap implementation; the
experimental tracing decorator retains command provenance without duplicating
the interpreter. See [`TRACING.md`](TRACING.md) for the current vertical slice
and intended trace semantics.

## Printer state

The mutable state contains only behavior required by implemented commands:

- active print area and horizontal position;
- motion units, line spacing, and tab stops;
- justification and text modes;
- selected font, code page, and international character set;
- barcode and QR settings;
- stored QR data and buffered graphics;
- the current Standard-mode line;
- completed and active roll surfaces; and
- non-printing device events such as a cash-drawer pulse.

`ESC @` restores implemented settings to the selected profile's defaults and
clears volatile data according to covered command behavior.

New state is added with the command that needs it. V1 does not reserve runtime
models for Page mode, macros, non-volatile resources, or printer state supplied
from outside the job.

## Printer profiles

Profiles provide behavior that cannot be derived from ESC/POS bytes:

- printable width and horizontal/vertical DPI;
- optional cutter geometry as the physical print-head-to-blade distance;
- horizontal and vertical motion units;
- `ESC *` 8-dot vertical pitch for model-specific column-image geometry;
- model-specific positioning behavior for `ESC $` and `ESC \`;
- model-specific feed behavior for `ESC J`, an LF immediately following
  `GS v 0`, and `GS V` Function B modes;
- reset line spacing, code page, international set, and carriage-return mode;
- Font A/B cell size and baseline;
- imported or self-contained code-page slots;
- capabilities used by implemented command handlers; and
- exact `GS k` systems supported by Function A and Function B.

Each field is a descriptor (an intrinsic physical fact) or a deviation (a
confirmed departure from documented ESC/POS baseline behavior); every field
is optional, and stating one is itself the confirmation (DD-031). See
[`PROFILE_SCHEMA.md`](PROFILE_SCHEMA.md) for the full model.

Physical profiles use the upstream `escpos-printer-db` repository as a Git
submodule. Its gitlink pins the complete upstream snapshot. Each upstream
profile source also stores the SHA-256 of its resolved profile, so a change
affecting that printer requires review.

`REFERENCE` is a separate virtual source. It imports nothing from the printer
database and explicitly supplies every current capability and code-page slot.
It represents documented baseline behavior without printer-specific
restrictions. Its 203 DPI, 576-dot paper and cutter geometry are concrete
virtual rendering parameters, not universal ESC/POS mechanism dimensions.

Profile authoring and calibration assets are collocated in visible
`crates/escpost-profiles/profiles/<profile-id>/` directories. A physical profile also contains the
expected rendering and physical verification of `crates/escpost-profiles/calibration-job.hex`.
Virtual profiles use focused automated golden cases instead of claiming
physical evidence. Hidden `.escpos-printer-db/` and `.generated/` directories
contain infrastructure, not profiles.

The profile compiler either combines upstream capabilities with a typed TOML
enrichment or compiles a self-contained virtual source. It generates the same
canonical JSON shape for both. The renderer loads only that generated profile;
it does not read the upstream database or TOML at render time.

A profile that advertises full- or partial-cut support must define
`cutter.print_head_to_cutter_dots`. `GS V` Function B uses that fixed distance
plus its command-supplied vertical-motion-unit feed before creating the sheet
boundary. A profile without an autocutter omits the cutter section; Function B
then applies only its profile-selected explicit feed behavior.

Each canonical profile carries:

- a typed source — `Reference`, curated `Upstream`, or synthesized
  `UpstreamDefault`; and
- a canonical-profile SHA-256 covering every runtime field.

The canonical hash is the profile's rendering identity. Manually maintained
profile revisions and duplicate repository provenance are intentionally absent.

## Text and symbols

Printable bytes are decoded with the profile-selected code page and Epson
international-character substitutions. The bundled Noto Sans Mono font is
rasterized deterministically into profile-defined character cells. Font engine
advance widths never move the print cursor.

One-dimensional barcode encoders return logical bar and space elements. The
printer state remains responsible for module scaling, placement, HRI, and paper
advance.

QR generation is isolated behind a small adapter around the pure-Rust `qrcode`
crate. The adapter returns an unscaled Boolean module matrix; it cannot place or
render receipt content.

## Dot surfaces and sheets

Surface code is divided into the private rendering contract, the canonical
`MonoSurface`, and an experimental tracing decorator. Ordinary rendering
selects `MonoSurface` statically and carries no trace records; traced rendering
wraps the same raster implementation and is opt-in.

`MonoSurface` stores one byte of ink coverage per scaled subpixel. Faithful
rendering thresholds glyph coverage to hard dots and encodes a one-bit
grayscale PNG. Optional antialiased preview rendering retains soft glyph
coverage and encodes an eight-bit grayscale PNG. Dot-space graphics remain
hard-edged in both modes.

A cut finalizes the active surface. Later output starts another sheet. Without
a cut, final sheet height follows painted content and paper-feed position.

Each `RenderedSheet` contains the logical surface and its encoded PNG. Tests
inspect faithful surfaces for exact command behavior and decode their one-bit
PNGs for end-to-end fixtures.

Additional color or tone models will be designed when an implemented command
requires them. V1 carries no unused color-plane abstraction.

## Results

Successful rendering returns:

```text
RenderResult
├── sheets
├── device_events
├── warnings
└── metadata
    ├── renderer version
    ├── profile id
    └── canonical profile SHA-256
```

Warnings are non-fatal diagnostics from an otherwise successful render, such
as a cut requested on a profile whose printer has no cutter. Known fidelity
boundaries of the renderer itself — representative glyphs, QR mask choice,
unmodeled thermal artifacts — are documented divergences (DD-002, DD-007,
DD-023, DD-024, DD-025) rather than a render-time channel; a profile's
`source` marker signals whether its own descriptors and deviations are
calibrated or synthesized (`PROFILE_SCHEMA.md`).

Device events describe supported non-printing commands and do not make the PNG
incomplete. Callers that care about those actions inspect the event list.

## Resource safety

Rendering limits apply before or during allocation:

- input bytes;
- declared command payload bytes;
- sheet width and height;
- sheet count; and
- total rendered dots.

Limit violations return a controlled error. These limits remain part of v1
because the future API endpoint will accept untrusted print jobs.

## Extension rule

The long-term command target remains the Epson-documented ESC/POS set, tracked
in `COMMAND_COVERAGE.md`.

New protocol families should add the smallest state and profile fields needed
by their first tested vertical slice. The architecture does not pre-model
unimplemented commands.
