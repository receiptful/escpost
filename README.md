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
| **PNG and web preview** | Printer-resolution PNG previews, multi-sheet jobs, integer zoom, antialiasing, and file watching. |
| **Cloud printing** | Planned native integration with [Receiptful](https://receiptful.io); today, Receiptful is available separately for thermal-printer delivery, job history, and managed cloud printing. |

## Render and capture ESC/POS data

Install the ESCPost CLI from crates.io with Rust 1.89 or newer:

```bash
cargo install escpost
```

Homebrew installation will follow. See [Development](#development) to build
ESCPost from the source checkout.

Render raw ESC/POS bytes, readable hexadecimal input, or stdin to PNG:

```bash
escpost render receipt.bin \
  --profile REFERENCE \
  --output receipt.png \
  --non-interactive
```

Preview receipts and labels in your browser and rerender when the source
changes:

```bash
escpost render receipt.hex --profile REFERENCE --web --watch
```

Or run a virtual printer and point an application at the reported RAW TCP
address:

```bash
escpost serve
```

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
| [`escpost-python`](crates/escpost-python) | Call the Rust preview engine from Python and receive the rendered PNG sheets. |

The virtual IP printer, browser workbench, file watching, and USB or RAW TCP
printer management are currently CLI features; they are not yet exposed as
reusable library APIs.

## Development

Build, test, and run either natively or in Docker. Both workflows build the
embedded frontend before compiling the Rust binary:

- **Native** requires host Rust and Bun toolchains and produces a host binary.
  Use it for host-only behavior such as opening the browser automatically.
- **Docker** provides the reproducible environment used by tests and CI and
  requires neither toolchain on the host. It is the canonical workflow.

The [`justfile`](justfile) wraps both workflows:

| Task | Docker | Native |
|---|---|---|
| Build the CLI | `just docker-build` | `just native-build` |
| Run the tests | `just docker-test` | `just native-test` |
| Run the CLI | `just docker-run serve --no-open` | `just native-run serve` |
| Run Axum and Vite | `docker compose up` | `just native-web-dev` |

`docker compose up` is the complete development stack. `./escpost serve` and
`just docker-web-dev` are aliases for it. Backend source changes restart the
Rust process; Vite serves the frontend at `http://127.0.0.1:5173/app/` with hot
reload. The backend continues to serve the existing embedded viewer at
`http://127.0.0.1:9000/`. The `/app/` workbench does not replace that viewer.
It currently provides five read-only routes:

- `/app/` — Overview
- `/app/jobs` — Print jobs, with a link to the existing current-job viewer
- `/app/printers` — configured printer inventory
- `/app/profiles` — complete printer-profile catalog
- `/app/calibration` — calibration guidance

Jobs deliberately continues to link to the legacy viewer at `/`; a job
inspector is the next workbench milestone.

For a production-like run of the embedded frontend without development
watchers, use `docker compose run --rm -e ESCPOST_WATCH=0 escpost serve`.

Run `just --list` to see every recipe. Without `just`, each recipe is a short
wrapper around `docker compose` or `cargo` and can be run directly. The native
build produces `target/release/escpost`. To install that checkout on `PATH`:

```bash
cargo install --path crates/escpost
```

Additional tasks:

- `just pack` regenerates the canonical printer-profile pack.
- `just python-test` builds and exercises the Python binding.
- `./escpost` remains the containerized CLI entry point with USB access.

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
