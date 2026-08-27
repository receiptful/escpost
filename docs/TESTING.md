# Testing ESCPost

ESCPost uses deterministic automated tests and opt-in physical-printer
calibration. Both paths consume the same version-controlled ESC/POS byte
streams without changing their bytes.

The automated suite protects behavior on every development machine and in CI.
Physical printing establishes and checks model-specific behavior that cannot
be proven from documentation alone.

## Principles

1. Test observable behavior through the public rendering interface.
2. Give rendering and physical printing the same decoded ESC/POS bytes.
3. Compare decoded pixels or logical dot surfaces, never compressed PNG bytes.
4. Keep physical-printer tests explicit; ordinary test commands must never
   print paper.
5. Treat hardware observations as evidence for a selected profile, not as
   universal ESC/POS behavior.
6. Never accept a changed golden image only because the implementation
   produced it.

## Test layers

### Public behavior tests

Most regression tests call the public Rust rendering API with:

- ESC/POS input bytes;
- a resolved printer profile;
- explicit resource limits and render options where relevant.

They assert observable results:

- sheet count and dimensions;
- decoded output pixels or logical dots;
- feeds and cuts;
- device events;
- non-fatal render warnings; and
- reproducibility metadata.

These tests should survive refactoring of tokenizers, command handlers,
buffers, and surface storage.

Focused parser or state tests are appropriate for framing and transition
invariants, but they supplement rather than replace public behavior tests.

### Command-interaction tests

ESC/POS is stateful. A command can change how later text, graphics, symbols,
feeds, or cuts behave. Command-interaction tests prove these documented
relationships through the same public rendering interface as other behavior
tests.

Keep isolated command mechanics in a command-specific file. For example,
`render_gs_v0.rs` owns raster framing and scaling. Put shared-state behavior in
a file named after the governing state:

```text
crates/escpost-render/tests/
├── render_justification.rs
├── render_print_area.rs
├── render_initialization.rs
└── render_buffering.rs
```

This makes a failed interaction easy to find without creating one large
`render_interactions.rs` file. Each test should still explain and assert one
observable behavior.

[`crates/escpost-render/tests/INTERACTIONS.md`](../crates/escpost-render/tests/INTERACTIONS.md) is the coverage inventory.
Add an interaction when the ESC/POS reference says commands share state, not
merely because two commands could appear next to each other. Implement entries
one behavior at a time as their commands become supported.

### Python binding tests

Binding tests prove that Python callers receive the same behavior as Rust
callers. They cover:

- byte input and profile selection;
- PNG, warning, event, and metadata results;
- Rust error conversion to documented Python exceptions; and
- repeated or concurrent calls.

They should not duplicate the complete Rust conformance suite.

### Rust CLI and HTTP tests

`crates/escpost/tests/` exercises the developer command as a subprocess
and the web app over real loopback sockets. These tests cover:

- binary, hexadecimal, stdin, and case-directory inputs;
- explicit and metadata-supplied profile resolution;
- zero, one, and multiple sheets across file, stdout, directory, and web
  destinations;
- exact byte-clean PNG stdout and nonzero error results;
- output replacement and manifest publication;
- automatic, strict, and operating-system-selected web ports;
- ordered HTTP metadata, PNG responses, missing routes, and path traversal;
- simultaneous persisted and web output;
- successful and failed watched rerenders; and
- `serve` capturing a RAW TCP job, previewing the most recent one, and showing
  a waiting hint before the first job arrives.

Run them with:

```bash
docker compose run --rm test cargo test -p escpost
```

Browser verification uses the same Docker entry point and a real browser
against the printed loopback URL. Confirm responsive ordered sheets, their
labels and dimensions, 1× default scale, integer zoom, and watch refresh before
removing or materially changing an older preview path.

### Frontend tests

The frontend lives under `crates/escpost/frontend`. Run its component tests,
type checker, and production build through Docker:

```bash
docker compose run --rm frontend bun test
docker compose run --rm frontend bun run typecheck
docker compose run --rm frontend-build
```

During focused frontend work, run the affected component directly, for example:

```bash
docker compose run --rm --no-deps frontend bun test src/features/profiles/page.test.tsx
docker compose run --rm --no-deps frontend bun test src/features/printers/page.test.tsx
```

Frontend printer-inventory tests use a fake `EventSource` to cover the initial
checking state, complete unnamed inventory messages, invalid payload handling,
automatic-reconnect stale rows, warning visibility, provider lifetime across
navigation, and registration waiting for the next authoritative snapshot
without a list GET. The manual browser check complements those tests: confirm
that `/api/printers/list/events` supplies the initial rows, a reconnect keeps
rows and warnings visible, a registration appears in a later SSE snapshot,
navigation retains one inventory stream, and the final closed connection stops
backend probing before a resumed connection receives retained then fresh data.

The terminal check runs `escpost printers list --monitor` in a real TTY. Check
the initial checking copy, compact redraws, connected and unavailable changes,
`--transport` presentation filtering, configuration edits, and terminal
restoration after Ctrl+C. Also confirm that `escpost --non-interactive printers
list --monitor`, and redirected monitor stdout, fail with the interactive
terminal error.

Rust HTTP integration tests exercise the embedded production bundle, including
direct navigation to every workbench route, navigation
labels in the production bundle, asset MIME and cache headers, missing assets,
and traversal rejection. They also cover the read-only `/api/status`,
`/api/printers/list`, `/api/printers/list/events`, `/api/profiles/list`, and
current-job resource contracts. Printer-monitor tests use deterministic
collectors and clocks to cover first and last subscriber lifecycle, retained
snapshot then forced-fresh resumption, five-second collection, idle shutdown,
and registration-triggered refresh. The printer SSE contract tests verify that
the stream sends complete inventory snapshots as unnamed default `message`
events, matching the one-shot resource shape rather than introducing an
SSE-only envelope,
while confirming that unknown API routes stay JSON rather than falling back to
HTML. They also cover the discovery routes: `/api/printers/discover/networks`
lists detected and skipped adapters, and `/api/printers/discover` streams
`prepared`, `progress`, and `completed` server-sent events over a two-address
subnet that answers immediately, finds a stand-in printer on a loopback
address the scanning host does not hold, and refuses a malformed subnet, an
undeclared parameter, a network option on a USB-only scan, and a subnet wider
than the explicit `/16` limit. The first write route, `POST
/api/printers/add`, is covered too: it persists a printer and returns the
saved facts, carries the USB ambiguity advisory, answers `409` for a name
already configured, `400` for invalid facts and malformed bodies, and `405`
with an `Allow` header for a GET. Job-resource tests verify that a replaced
job identifier cannot resolve to a newer job. The existing viewer at `/` remains covered separately as a
behavioral reference during the SPA transition.

### Robustness tests

Malformed, truncated, adversarial, and resource-intensive streams verify that
the renderer returns controlled errors instead of panicking, hanging, or
allocating without bounds.

Fuzzing targets command framing and state-machine execution. A discovered
failure becomes a permanent minimal regression case before the implementation
is fixed.

### Network discovery without hardware

`printers discover` needs something on the network to find. A Compose profile
provides a virtual IP printer for that, so the scan can be exercised without a
physical device:

```sh
docker compose --profile dummy-printer up -d dummy-printer
docker compose run --rm test cargo run -p escpost -- printers discover --transport network
```

It answers on `172.31.42.2:9100` and reports as reachable via `escpost-dummy`.
Stop it with `docker compose --profile dummy-printer down`.

The service is the one place in `compose.yaml` that does not use host
networking, and that is deliberate rather than incidental: a scan never probes
the scanning machine's own addresses, so a listener sharing the host's network
namespace is excluded by design and could never be discovered. Giving the
printer a bridge of its own puts the host on the gateway address and the
printer on another, which is the shape discovery expects of a real printer on
a real segment. A scan of that subnet reports 253 addresses rather than 254,
because the gateway is the scanning machine and is correctly left out.

It is a real `escpost serve` listener, not a stub, so it also accepts jobs
printed to it.

### Physical-printer calibration

Hardware calibration sends either a focused case or the shared calibration
receipt to a selected printer and renders those same bytes with the matching
profile.

The first physically calibrated profile is `NT-5890K`, a Netum 58 mm printer.
The upstream profile inherits from `POS-5890`, currently describing:

- a 384-dot printable width;
- 203 DPI;
- 32 columns for Font A; and
- 42 columns for Font B.

These values are starting hypotheses. The connected printer and its
documentation determine whether ESCPost needs profile enrichments or
corrections.

The upstream NT-5890K profile inherits conservative native-symbol flags from
the generic `simple` profile. A raw hardware probe on the connected printer
successfully printed both `GS k` Function A/B EAN-13 symbols and a Model 2
`GS ( k` QR symbol. The enrichment therefore advertises the established
Function A/B systems and QR support. Newer model-dependent Function B systems
such as GS1-128 and automatic Code 128 remain absent until they are separately
verified on this printer. The exact stream and observation are retained in
`crates/escpost-render/tests/cases/symbols/native-symbols-nt-5890k`.

Separate raw probes for GS1 DataBar systems `m=75` through `m=78` printed no
bars or HRI on this firmware. Their payload bytes appeared as ordinary text,
showing that the printer does not recognize these values as length-prefixed
`GS k` commands. The exact probe streams are retained beside the successful
native-symbol case, and these four capabilities remain absent from the
NT-5890K enrichment.

Physical motion probes also established three model-specific feed behaviors:
`ESC J` is consumed without feeding, `GS V 65 n` feeds by the requested
amount, and `GS V 66 n` is consumed without feeding. These affect layout and
therefore live in the typed profile rather than being treated as incidental
print artifacts.

An isolated positioning-interaction probe established that the connected
NT-5890K applies positive `ESC \` movement, consumes negative `ESC \` without
moving, and consumes `ESC $` without moving after printable data is already
buffered on the line. These are typed profile behaviors because they change
the coordinates of later content.

An isolated raster/LF interaction probe established that the printer consumes
exactly one LF immediately following `GS v 0`. Zero and one LF leave adjacent
raster blocks, while two consecutive LFs produce one line feed. The renderer
keeps Epson's ordinary following-LF feed as its baseline and stores the NT
difference as a typed profile behavior.

## Conformance case format

Each behavior is represented by a self-contained case directory:

```text
crates/escpost-render/tests/cases/text/default-font/
├── case.toml
├── input.hex
├── expected-001.png
└── notes.md
```

`input.hex` is the canonical, diff-friendly serialization of the byte stream.
It contains whitespace-separated two-digit hexadecimal bytes. The case loader
strictly decodes it once and gives the same immutable byte buffer to the
renderer and physical-printer transport. Neither path may regenerate,
normalize, prefix, suffix, or otherwise transform those decoded bytes. Git
already versions the fixture, so the manifest does not duplicate an input
hash.

`case.toml` records only values the loaders consume:

```toml
schema_version = 1
name = "default Font A advances by 12 dots"
profile = "NT-5890K"
```

The input file is always `input.hex`. Expected sheets are discovered as
`expected-001.png`, `expected-002.png`, and so on. Their decoded dimensions are
the authority, so the manifest does not duplicate filenames or sizes.
References and physical observations belong in `notes.md`.

`expected-001.png` is a lossless, reviewable representation of expected dots.
Tests decode it and compare its pixel values and dimensions. PNG encoder output
bytes are not asserted because compression settings can change without
changing the receipt.

Golden conformance tests always preserve each newly rendered sheet under:

```text
.test-output/<case-path>/actual-001.png
```

The output directory mirrors `crates/escpost-render/tests/cases/`, uses the same three-digit sheet
numbers, and is ignored by Git. This lets developers inspect successful
renders as well as failures.

Shared calibration outputs use
`.test-output/calibration/<profile-id>/actual-001.png`.

`escpost render --output-dir` writes `sheet-NNN.png` and a `manifest.json`
whose `sheets` array lists those files in receipt order. The manifest is the
authority, so stale unlisted PNGs are ignored.

When pixels differ, the test also writes `diff-001.png` and
`comparison.html`. Matching ink is black, matching paper is white, unexpected
ink is red, and missing ink is blue. The assertion reports repository-relative
paths for the expected, actual, diff, and comparison files together with the
changed-dot count and difference bounds.

Tests never rewrite expected PNGs. A developer reviews the generated actual
image and accepts a new golden explicitly.

Run only the golden conformance layer with:

```bash
docker compose run --rm test \
  cargo test -p escpost-render --test golden_cases -- --nocapture
```

`notes.md` explains the behavior under test, relevant commands, manual
references, intentional documented divergences, and physical observations. It
should not be required for executing the test.

Cases with multiple cuts contain one expected PNG per sheet.

Compatibility cases under `crates/escpost-render/tests/cases/compatibility/` pin byte streams
generated by real upstream producers. For example,
`receiptful-html2escpos` is generated once through Receiptful's public
HTML-to-ESC/POS entry point and then rendered as immutable input here. The
standalone library never imports producer code at runtime. If an emitter
changes intentionally, regenerate its stream in the producer's own build
environment and review the PNG difference before updating the compatibility
case.

## Shared profile calibration

Focused cases isolate one behavior for automated diagnosis. Physical-printer
calibration instead uses one comprehensive stream for every physical profile:

```text
crates/escpost-profiles/
├── calibration-job.hex
├── CALIBRATION.md
└── profiles/
    └── <profile-id>/
        ├── profile.toml
        ├── expected-001.png
        ├── verification.toml
        ├── notes.md
        └── TODO.md              # only when hardware work remains
```

The shared stream exercises the broad supported receipt surface. Do not fork
it per printer. Each physical profile's expected PNG captures the layout
produced by that profile, while smaller cases remain the primary way to locate
regressions.

Virtual profiles do not carry `verification.toml` or claim comparison with
paper. `REFERENCE` instead uses the focused
`crates/escpost-render/tests/cases/mechanism/reference-full-and-partial-cuts` golden case. Its two
cuts must produce `actual-001.png`, `actual-002.png`, and `actual-003.png` in
that exact manifest order.

`verification.toml` contains only:

```toml
renderer_commit = "<full 40-character Git commit>"
last_verified = "YYYY-MM-DD"
```

The renderer commit says which renderer behavior was compared with the
physical output. The containing repository commit versions `profile.toml`,
the shared stream, expected PNG, notes, and verification record together.
Input and profile hashes would duplicate that history.

When a renderer change alters a calibrated PNG, review the automated diff and
compare the new output with the physical printer before advancing
`renderer_commit` and `last_verified`. If the printer is unavailable, record
the pending checks in that profile's `TODO.md` instead of claiming a new
verification.

## Physical calibration commands

Physical calibration reuses the general-purpose Rust commands; there is no
dedicated calibration command group. Register the printer once, then render and
print the same version-controlled input — a conformance case, or the shared
receipt at `crates/escpost-profiles/calibration-job.hex`:

```text
escpost printers add <local-name> --transport usb \
  --vendor-id <VID> --product-id <PID> --profile <profile>
escpost render <case-or-input> --profile <profile> --output-dir <dir>
escpost print <case-or-input> --printer <local-name>
```

`render` rasterizes the input to the actual PNG sheets. `print` sends the
decoded input bytes unchanged to the selected physical transport. Both load the
same immutable source, so the previewed and printed bytes cannot diverge.

Use `escpost render <SOURCE> --web` for visual inspection. Its Rust web server
holds the rendered sheets in memory, labels them in order, wraps them when
space permits, and scales only by integer multiples so individual printer dots
remain inspectable. `--watch` updates the view after successful filesystem
changes while retaining the last complete render after an error.

The normal read-only inventory is native Rust:

```bash
./escpost printers list
```

Its unit tests substitute the USB inventory boundary, verify the exact
connection fields shown to developers, and parse synthetic USB configuration
descriptors to exclude non-printer and non-bulk endpoints. The connected
printer smoke check verifies the `nusb` enumeration path without claiming an
interface or printing paper. A loopback integration test verifies that a
configured network endpoint moves between connected and unavailable as its
listener appears and disappears, and that the reachability handshake sends
zero bytes. Configuration tests verify explicit-path and environment
precedence, Linux/XDG platform resolution, configured-name matching,
unavailable saved printers, connected/configured de-duplication, status-first
display-name ordering, and the absence of filesystem writes during listing.
USB registration tests use the same synthetic inventory to prove that
configured devices are excluded, multiple bulk OUT endpoints stay explicit,
and the selected stable descriptor coordinates are serialized. They never
claim an interface or write bytes. Name-conflict tests prove that interactive
registration accepts a replacement while non-interactive registration fails
without modifying the existing configuration. Network-registration tests
cover an interactive port answer, bypassing the prompt with `--port`, and the
silent non-interactive `9100` default.

## Rust direct-print smoke tests

Use `escpost print` when an exact byte stream should be sent to a named
configured printer. Unlike rendering, this does not require or apply a profile:

```bash
./escpost print example-jobs/rust-print-smoke.hex \
  --printer netum-usb \
  --non-interactive
```

The committed smoke stream is intentionally short and reviewable, identifies
itself in plain text, and contains no cut command so it is safe for printers
without a cutter. The command reports the selected target and transferred byte
count, but never logs the receipt payload.

Automated `print` tests use a recording transport at the USB boundary and real
loopback TCP listeners for network output. They exercise source decoding,
configuration lookup, interactive selection and addition, target validation,
and exact byte preservation. Device-selection tests call the pure selection
helper with zero or several synthetic matches. Never add an automated test
that can resolve and write to a developer's configured physical printer.

## Local printer configuration

When commands run through the development wrapper, connection details belong
in the Compose-managed configuration volume. Add them through the CLI:

```bash
./escpost printers add
```

The resulting `printers.toml` has this shape:

```toml
[netum-usb]
transport = "usb"
profile = "NT-5890K"
vendor_id = "<USB vendor ID>"
product_id = "<USB product ID>"
# serial_number = "<optional USB serial number>"
interface_number = 0
out_endpoint = "0x01"
in_endpoint = "0x81"
```

Use `escpost printers add` for new USB and network entries. It preserves
comments and other printer entries and shares the legacy calibration CLI's
resolved configuration path. The named volume keeps Docker configuration
separate from the host and avoids bind-mount ownership differences. Local
captures remain ignored.

An installed native CLI instead uses the platform user-configuration
directory. Tests and automation can select an isolated directory with
`ESCPOST_CONFIG_DIR` or an exact file with `printers --config <FILE>`.

Before sending bytes, the CLI shows:

- the selected case or shared calibration input;
- printer profile;
- USB identity or other transport destination;
- byte count.

The explicit `print` command is the authorization to perform the physical
action. Automated tests and build scripts never invoke it.

The CLI adds no implicit initialization, feed, or cut commands. A case that
requires `ESC @`, trailing feed, or a cut includes those bytes explicitly in
`input.hex`.

## Calibration workflow

For each new visible behavior:

1. Add one conformance case that describes the public behavior.
2. Run it and observe the expected automated test failure.
3. Implement only enough behavior to make that case pass.
4. Run the complete automated suite.
5. If the behavior is model-sensitive, `render` and `print` the same case on
   the Netum printer.
6. Compare physical geometry with the rendered PNG.
7. Record the observation in `notes.md` and update the profile enrichment when
   the behavior is model-specific.
8. Refactor only while all automated tests remain green.

This is repeated one vertical slice at a time. Do not write a large suite of
speculative command tests before exercising the first command end to end.

For a new printer profile, first complete `profile.toml`, then `render` and
`print` `crates/escpost-profiles/calibration-job.hex`. Compare the one long physical receipt with the
profile's generated PNG, accept it as `expected-001.png`, record evidence in
`notes.md`, and write `verification.toml`. This broad calibration complements
the focused workflow; it does not replace it.

When the printer has an autocutter, add its physical geometry to the profile:

```toml
[cutter]
print_head_to_cutter_dots = 80
```

Use the model manual as the starting value, converting the documented
print-head-to-blade distance to printer dots. Then compare the Function B
sections of the shared calibration receipt. Full and partial cuts must put the
following marker on a new rendered sheet, and the preceding sheet must include
the fixed cutter distance plus the command's explicit feed. Record a pending
hardware check instead of guessing when the distance is not known.

## Comparing PNG and paper

Initial calibration may be visual, but test receipts should make discrepancies
easy to identify. Useful fixtures include:

- horizontal and vertical dot rulers;
- boundary marks at the printable area's edges;
- repeated characters that reveal cell width and wrapping;
- baseline and line-spacing patterns;
- aligned raster blocks with known dimensions; and
- labels identifying the section or command under test.

Display PNGs only at integer nearest-neighbor scales so individual logical
dots remain visible.

For a more objective comparison, scan the receipt flat at a known resolution,
then deskew, crop to registration marks, resample to the printer's nominal dot
grid, and threshold it. Physical output includes feed tolerances, thermal
spread, and scanning distortion, so the digitized receipt is evaluated with
documented tolerances rather than required to equal the logical raster bit for
bit.

The unprocessed scan or photograph is evidence, not an automated golden image.
If reference captures are retained, store their printer identity, firmware or
self-test information, configuration, capture method, date, and repository
commit.

## What hardware observations may change

Hardware evidence can justify:

- correcting profile geometry or defaults;
- documenting a firmware or compatibility-mode variant;
- adding a model-specific command quirk;
- correcting a profile capability or a documented divergence; or
- filing an upstream printer-database correction.

It does not justify changing Epson command framing or another model's behavior
without corresponding documentation or evidence.

If documentation and hardware disagree, retain both references and describe
the observed printer configuration. A new profile or firmware variant is often
safer than silently changing behavior for every device with the same marketing
name.

## Golden-image review

Golden images are updated deliberately:

1. Explain which documented behavior or physical evidence changed.
2. Render the affected case to a separate actual-output path.
3. Review dimensions, pixel differences, documented divergences, and
   unrelated regions.
4. Replace the golden only after the new result is accepted.
5. Commit the input, manifest, expected image, and notes together.

A bulk "regenerate all goldens" command must not overwrite expected files
without an explicit acceptance step.

## When hardware testing is required

Run applicable physical cases before accepting changes to:

- printer-profile geometry or defaults;
- text cell metrics, baselines, spacing, or wrapping;
- motion-unit conversion and rounding;
- raster, barcode, or two-dimensional-code placement;
- Standard-mode composition;
- feed, cutter, or sheet-boundary behavior; and
- model-specific commands or quirks.

Parser refactors, error-reporting changes, packaging, and equivalent PNG
compression changes normally require the automated suite but not new paper,
provided their existing conformance cases remain unchanged.

Contributors without the target hardware may still submit changes and fixtures.
They mark hardware validation as pending so a maintainer with the reference
printer can perform it.

## Reporting a physical run

A hardware-validation report includes:

```text
case or calibration profile:
renderer commit:
last verified date:
printer model:
firmware/configuration:
connection:
result:
observations:
capture, if any:
```

This is sufficient to reproduce the comparison and prevents an unexplained
"looks correct on my printer" from becoming profile behavior.
