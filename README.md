<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme/hero_dark.png">
    <source media="(prefers-color-scheme: light)" srcset="docs/assets/readme/hero_light.png">
    <img src="docs/assets/readme/hero_light.png" alt="ESCPost — the most complete ESC/POS developer toolbox" width="640">
  </picture>
</p>

ESCPost is a Rust-based command-line toolbox and reusable library for building,
testing, and debugging ESC/POS integrations. Render ESC/POS receipt and label
data without a printer, or connect an ERP or POS application to ESCPost as if
it were a network printer. Capture and preview its print jobs, then print to
physical USB and network printers from the same CLI.

## Your complete ESC/POS Toolbox

<p align="center">
  <img src="docs/assets/readme/features.png" alt="Overview of ESCPost's six core features" width="800">
</p>

| Feature | What it provides |
|---|---|
| **CLI and libraries** | A Rust CLI, reusable crates for processing ESC/POS data and printer profiles, and a Python API. |
| **Virtual IP printer** | Redirect an ERP or POS application to ESCPost as a RAW TCP network printer, then capture and inspect its print jobs in your browser, in real time. |
| **USB and IP printers** | Automatically discover connected USB printers, configure IP network printers, and test and calibrate each printer's profile. |
| **Printer profiles** | Device-specific geometry, capabilities, defaults, and calibrated behavior. |
| **PNG and web preview** | Printer-resolution PNG output, multi-sheet jobs, integer zoom, antialiasing, and browser inspection through the virtual printer. |
| **Cloud printing** | Planned native integration with [Receiptful](https://receiptful.io); today, Receiptful is available separately for thermal-printer delivery, job history, and managed cloud printing. |

## Render and capture ESC/POS data

Install the ESCPost CLI with Homebrew:

```bash
brew install receiptful/tap/escpost
```

Or from crates.io with Rust 1.89 or newer:

```bash
cargo install escpost
```

See [Development](#development) to build ESCPost from the source checkout.

Render raw ESC/POS bytes, readable hexadecimal input, or stdin to PNG:

```bash
escpost render receipt.bin \
  --profile REFERENCE > receipt.png
```

To preview jobs in the browser, start the virtual printer and workbench:

```bash
escpost serve \
  --listen 127.0.0.1:9100 \
  --web-listen 127.0.0.1:9000 \
  --profile REFERENCE
```

Send an existing ESC/POS source directly to that virtual printer:

```bash
escpost print receipt.hex --network 127.0.0.1:9100
```

### Raw browser printing

To expose the local print API without the embedded web application, run the
existing `serve` command with its web listener and API-only mode:

```bash
escpost serve --web-listen 127.0.0.1:9000 --no-web-app
```

For a printer configured with the exact name `counter`, submit the ESC/POS
file as raw bytes:

```bash
curl -X POST 'http://127.0.0.1:9000/api/print?printer=counter' \
  -H 'Content-Type: application/octet-stream' \
  --data-binary @receipt.bin
```

The `printer` value must be the exact configured printer name. Requests from
ordinary browser origins are rejected; browser-extension origins and local
program calls are allowed. This origin filter constrains which browser pages
may send a request; it is not authentication.

<p align="center">
  <img src="docs/assets/readme/web-preview.svg" alt="Placeholder for the ESCPost browser workbench" width="100%">
</p>

## Supported ESC/POS features

ESCPost currently implements all commonly used ESC/POS commands, including
profile-driven text and layout, common single-byte code pages, bit and raster
images, native one-dimensional barcodes, GS1-128, automatic Code 128, Model 2
QR codes, feeds, and cuts. Supported cuts produce separate ordered sheets.

Previews reproduce printable geometry and precise placement at the printer's
native resolution—not paper texture or an exact reproduction of proprietary
printer ROM glyphs. Use the virtual `REFERENCE` profile for generic previews,
or a physical profile for device-specific geometry and capabilities.

See [command coverage](docs/COMMAND_COVERAGE.md) for the detailed implementation
and validation matrix.

## Libraries

The CLI is backed by reusable Rust crates for applications that need to process
ESC/POS data directly, plus a Python binding to the same preview engine. The
Rust crates are published on crates.io; the Python package remains available
from the source workspace and has not yet been published to PyPI.

| Library or API | Available today |
|---|---|
| [`escpost-render`](crates/escpost-render) | Convert ESC/POS data into ordered PNG sheets using a selected printer profile. Results include warnings, device events, and reproducible profile information. |
| [`escpost-profiles`](crates/escpost-profiles) | Resolve profiles from the embedded catalog, inspect profile capabilities, and read, write, compile, or synthesize canonical profiles. |
| [`escpost-python`](python) | Call the Rust preview engine from Python and receive the rendered PNG sheets. |

The virtual IP printer, browser workbench, and USB or RAW TCP printer management
are currently CLI features; they are not yet exposed as reusable library APIs.

## Development

Docker Compose is the canonical development workflow and requires neither Rust
nor Bun on the host. Start the complete backend and frontend stack with:

```bash
docker compose up
```

Backend source changes restart the Rust process. Vite serves the frontend at
`http://127.0.0.1:5173/` with hot reload and proxies API requests to the
backend. To provide a discoverable RAW TCP printer without physical hardware,
run this in another terminal:

```bash
docker compose up dummy-printer
```

Use the root wrapper to run the containerized CLI with USB access:

```bash
./escpost printers list
```

Run the Rust workspace tests in the same reproducible environment:

```bash
docker compose run --rm test cargo test --workspace
```

The workbench provides five read-only routes:

- `/` — Overview
- `/jobs` — current print job, sheets, command trace, and annotations
- `/printers` — configured printer inventory
- `/profiles` — complete printer-profile catalog
- `/calibration` — calibration guidance

For a production-like run of the embedded frontend without development
watchers, use `docker compose run --rm -e ESCPOST_WATCH=0 escpost serve`.

Native development requires Rust and Bun on the host. `just` is optional, but
we recommend installing it to run the repository tasks below; their underlying
commands can also be invoked directly:

| Task | Command |
|---|---|
| Build the release CLI | `just build` |
| Run the tests | `just test` |
| Run the CLI | `just run serve` |
| Run the development stack | `just dev` |

The native build compiles the frontend first and produces
`target/release/escpost`. To install that checkout on `PATH`:

```bash
cargo install --path crates/escpost
```

`just dev` provides Vite HMR and uses Watchexec, when available, to restart the
Rust server after backend changes. If Watchexec is missing, it offers to install
it locally under `target/dev-tools`.

Additional tasks:

- `just docker-cargo-clean` clears the shared container build cache.
- `just generate-profile-pack` regenerates the canonical printer-profile pack.
- `cd python && just test` builds and exercises the Python binding.
- `just --list` shows every recipe.

## Documentation

- [CLI reference](docs/CLI.md) — inputs, output modes, commands, and automation behavior
- [Command coverage](docs/COMMAND_COVERAGE.md) — implemented protocol surface and validation
- [Printer profiles](docs/PROFILE_SCHEMA.md) — profile schema, enrichment, and corrections
- [Architecture](docs/ARCHITECTURE.md) — crate boundaries and render pipeline
- [Command tracing](docs/TRACING.md) — tracer architecture and command-effect semantics
- [Platform support](docs/PLATFORMS.md) — release targets and transport caveats
- [Testing and calibration](docs/TESTING.md) — conformance cases, golden images, and physical printers
- [Design decisions](docs/DESIGN_DECISIONS.md) — accepted technical decisions and rationale
- [Roadmap](docs/TODO.md) — planned developer-tool capabilities

## License

ESCPost code and documentation are licensed under the
[Apache License 2.0](LICENSE). Bundled third-party assets retain their own
licenses and attribution.

ESC/POS is a registered trademark of Seiko Epson Corporation. ESCPost is an
independent open-source project and is not affiliated with or endorsed by
Epson.
