# Design Decisions

This is the project's durable domain-decision log. It records choices about
ESC/POS interpretation, rendering, fidelity, printer behavior, safety, and
other principles whose rationale cannot be recovered from the current
architecture alone.

Current component boundaries and implementation structure belong in
`ARCHITECTURE.md`. Testing workflow belongs in `TESTING.md`; profile-format
details in `PROFILE_SCHEMA.md`; and repository, tooling, licensing, release,
and contribution process in their corresponding project documents.

Decision numbers are stable and may contain gaps when an entry is removed,
merged, or relocated outside this document's scope.

Each decision has a status:

- **Accepted**: a settled decision the project follows.
- **Provisional**: a current direction, deliberately easy to revisit.

Entries are not immutable. As the design evolves, edit, merge, or remove them
directly — git preserves the history, so the document need not carry a record of
how a decision changed.

## Authoring entries

Entries capture higher-level design decisions about ESC/POS receipt printing,
rendering, and fidelity — the concepts and methodology behind the project.
Architecture and implementation structure belong in `ARCHITECTURE.md`, not here.

Write each one as a timeless statement of the decision and its rationale. An
entry describes the design as though it always held: state the situation and the
choice in the present tense. Avoid before/after narration — no "previously",
"used to", "now", or "as before". Git preserves how the design evolved, so the
entry need not recount it.

Each entry follows the same shape:

- a `## DD-NNN — <short title>` heading and a `**Status:**` line;
- **Context** — the situation and constraints that force a choice;
- **Decision** — what is decided, in the present tense;
- **Consequences** — what the decision commits the project to, good and bad.

## DD-002 — Promise geometry fidelity, not photographic fidelity

**Status:** Accepted

### Context

An exact physical reproduction would require proprietary font ROMs, firmware
details, paper chemistry, print density, print-head condition, and mechanical
tolerances. The product need is an accurate preview of layout and printed
elements.

### Decision

Target near dot-perfect geometry for the selected printer profile: positions,
dimensions, cell advancement, wrapping, print areas, buffers, feeds, and cuts.

Exact proprietary glyph shapes and physical print artifacts are outside the
initial fidelity contract.

### Consequences

- The project can use redistributable, deterministic glyph sources.
- Character-cell metrics remain part of the fidelity promise.
- Documentation must not market the result as a photographic printer
  simulation.

## DD-003 — Use a dot-addressed raster as the canonical result

**Status:** Accepted

### Context

ESC/POS printers ultimately energize or strike dots. Using HTML, CSS, SVG text,
or a browser layout engine would delegate important geometry to a renderer
whose behavior is not controlled by the printer profile.

### Decision

The virtual printer renders to one or more dot-addressed sheet surfaces.
Output encoders consume those surfaces.

### Consequences

- Browser layout and font metrics cannot move receipt elements.
- Raster graphics map naturally to the result.
- Memory and dimension limits are required.
- Alternate encoders remain possible without changing printer emulation.

## DD-004 — Make PNG the primary output

**Status:** Accepted

### Context

The canonical result is already a raster. PNG is lossless, broadly supported,
compact for typical receipt imagery, and easy to display on Android and the
web.

### Decision

PNG is the primary public rendering format. Monochrome output should use a
one-bit representation where practical. Integer scaling may be offered for
high-density displays.

### Consequences

- No headless browser is required.
- PNG compression bytes are not suitable as golden fixtures; decoded pixels
  are.
- A future SVG encoder is optional rather than architectural.

## DD-005 — Keep the coordinate system fixed in printer dots

**Status:** Accepted

### Context

Printer commands can change motion units, margins, active print areas, and Page
mode directions. These change the interpretation of command parameters, not
the physical resolution of the print head.

### Decision

Use immutable horizontal and vertical printer-dot coordinates supplied by the
profile. Convert command motion units into that coordinate system using
model-specific truncation and mechanical-pitch rules.

### Consequences

- `GS P` and related commands mutate state without resizing the surface width.
- Horizontal and vertical DPI and motion defaults must be independently
  representable.
- Rounding behavior belongs to command/profile semantics.

## DD-006 — Treat printer profiles as content-addressed behavioral inputs

**Status:** Accepted

### Context

ESC/POS command support, parameter ranges, defaults, print geometry, code-page
mappings, font metrics, storage, and quirks differ by model and sometimes by
firmware or configured compatibility mode.

### Decision

Rendering always uses an explicit printer profile covering behavior as well as
geometry. The canonical content hash is the exact profile identity; no manual
profile revision is maintained.

### Consequences

- There is no unqualified, universally accurate "ESC/POS default printer."
- Callers can reproduce historical previews by retaining the profile id and
  canonical hash.
- Profile validation and conformance fixtures are first-class project work.

## DD-007 — Match character metrics without cloning resident glyphs

**Status:** Accepted

### Context

Exact printer ROM glyphs are unnecessary for the layout-preview goal and may
not be available as redistributable assets. Font engine metrics must still not
control ESC/POS advancement.

### Decision

Decode characters according to printer state, rasterize deterministic
representative glyphs, and fit them into profile-defined cells and baselines.
Advance using ESC/POS metrics only.

### Consequences

- Text layout can be geometry-faithful even when glyph shapes differ.
- Host-installed fonts are unsuitable.
- The glyph provider is replaceable so profiles may later supply exact bitmap
  atlases.
- Broad script coverage and asset licensing remain implementation concerns.

## DD-008 — Target the full documented Epson ESC/POS set incrementally

**Status:** Accepted

### Context

Receiptful initially needs a limited command subset, but an open-source
renderer should not be architecturally restricted to commands emitted by one
encoder.

### Decision

The long-term protocol target is the full Epson-documented ESC/POS command set,
including model-specific behavior, Standard mode, Page mode, downloaded and NV
resources, native symbols, color/tone graphics, and non-visual device actions.

Implementation and release coverage grows incrementally. Non-Epson extensions
are not implied by the initial support claim.

### Consequences

- Each command family adds the smallest framing, state, and profile model needed
  by its first tested vertical slice.
- A support matrix is required.
- "Full support" is evaluated for a selected profile, because individual
  printers intentionally support only subsets.

## DD-009 — Implement monochrome before modeling additional color

**Status:** Accepted

### Context

Most thermal receipt printing is one bit per dot. The full Epson graphics
functions also include multiple-tone data with four weighted planes, and some
models support spot colors such as black and red.

### Decision

V1 uses one printed/not-printed surface. Multiple-tone or spot-color
representations will be designed with the first implemented command that needs
them instead of reserving an unused abstraction.

### Consequences

- The implemented surface matches current one-bit command semantics directly.
- Future color work may extend or replace the surface representation based on
  concrete command and printer evidence.

## DD-010 — Emulate buffers and state instead of translating commands directly

**Status:** Accepted

### Context

ESC/POS commands form a stateful instruction stream. Alignment can apply to a
composed line, Page mode buffers data before printing, and resources can be
defined in one command and printed later.

### Decision

Interpret implemented commands through a virtual-printer state machine with
Standard-mode line composition and the resource stores those commands require.
Do not translate each command independently into final pixels. Add Page mode
state when Page mode becomes an implemented vertical slice.

### Consequences

- Command ordering and reset behavior can be represented correctly.
- State that is not part of the submitted isolated job is outside v1.
- State transitions need extensive unit tests.

## DD-011 — Represent cuts as sheet boundaries

**Status:** Accepted

### Context

A byte stream may feed and cut paper multiple times. A single unbounded bitmap
does not represent the physical result cleanly.

### Decision

The render result is a sequence of sheets. A cut finalizes the active sheet and
starts the next one when subsequent printable output appears. Cutter-equipped
profiles store the physical print-head-to-blade distance in dots. `GS V`
Function B advances by that distance plus its `n × vertical motion unit`
operand before finalizing the sheet. On a printer without an autocutter,
Function B performs only the explicit feed selected by that profile.

### Consequences

- One job may produce multiple PNG files.
- Feed-to-cutter behavior affects sheet height.
- Cutter geometry is required whenever a profile advertises a cut capability.
- Non-cut jobs finalize at the final content/feed position.

## DD-012 — Fail explicitly and never guess unsafe framing

**Status:** Accepted

### Context

Unknown, unsupported, malformed, or truncated commands are unavoidable,
especially with vendor extensions. Guessing where a binary payload ends can
desynchronize the remainder of the stream and create a misleading preview.

### Decision

V1 returns a structured `RenderError` for malformed, truncated, unknown, or
unsupported input. It does not continue with a partial preview after an error.
Binary payloads are consumed only through documented framing.

### Consequences

- Errors retain byte offsets and command identity when known.
- A future partial-preview mode requires concrete recovery semantics and a new
  result model.

## DD-013 — Enforce explicit resource limits

**Status:** Accepted

### Context

ESC/POS inputs can declare large raster dimensions, retain resources, execute
macros, create long feeds, and otherwise cause excessive CPU or memory use.

### Decision

Apply configurable limits with conservative defaults to input size, payload
size, rendered dots, sheet dimensions/count, stored resources, symbol data,
and repeated execution.

### Consequences

- Limit violations are controlled errors, not crashes or unbounded allocation.
- Applications may select stricter limits for untrusted public input.
- Tests must include adversarial streams.

## DD-015 — Make rendering assumptions reproducible

**Status:** Accepted

### Context

The same byte stream can render differently after a profile correction or
renderer behavior change. A physical printer may also have state established
before an isolated job.

### Decision

V1 defines every submitted byte stream as an isolated job starting from the
selected profile's reset defaults. A result records the renderer version,
profile id, and canonical profile hash.

### Consequences

- Reproducing a historical preview requires more than retaining its ESC/POS
  bytes.
- Applications can include renderer version and canonical profile hash in cache
  keys.
- Device-resident state is outside the v1 input model rather than represented
  by an unused snapshot abstraction.

## DD-018 — Import and enrich the shared ESC/POS printer database

**Status:** Accepted

### Context

`receipt-print-hq/escpos-printer-db` is already consumed by python-escpos and
provides community-maintained profile identifiers, media dimensions, DPI,
font columns, code pages, colors, and capability flags. Recreating that catalog
would fragment identifiers and duplicate maintenance. Its schema does not,
however, describe all geometry and behavior required by an emulator.

### Decision

Track the upstream database as a source repository and import it at build time.
Maintain ESCPost enrichment files that state exact descriptors and the
behavioral deviations a printer confirms (DD-031). Resolve and validate both
sources into a canonical profile pack embedded in the Rust library.

Do not fetch profile data at installation or render time. The selected Git
submodule revision supplies the build input, and the canonical content hash
identifies the resulting runtime profile. Per-profile input hashes do not
duplicate Git and code review as change-approval mechanisms.

### Consequences

- python-escpos generators and ESCPost previews can share profile names.
- Receiptful custom profiles can feed both systems.
- Upstream updates are ordinary reviewable dependency changes.
- The renderer is insulated from upstream schema changes by its importer and
  canonical internal schema.
- A large catalog does not imply high-fidelity support: a profile without
  enrichment rests on default base values and is marked as synthesized rather
  than calibrated (DD-031, DD-032).

## DD-022 — Use typed profile enrichments

**Status:** Accepted

### Context

ESCPost must complete and occasionally correct the shared upstream printer
database. A mature per-field evidence and patch protocol would provide strong
audit detail but would impose substantial authoring and implementation cost
before the first profile is calibrated.

Git and pull-request review already record and review changes to both upstream
inputs and local enrichments. A second per-profile approval mechanism would
duplicate that workflow and complicate the source model.

### Decision

Express enrichments as typed TOML with simple source references, and generate
deterministic canonical JSON with a canonical profile hash.

Reject unknown enrichment fields and invalid resolved values. Do not store or
check per-profile hashes of upstream inputs; upstream changes flow into the next
generated runtime pack and are reviewed as ordinary Git changes. Defer a generic
patch language, operation declarations, separate evidence records, per-field
provenance wrappers, and numeric confidence values until real maintenance needs
require them.

### Consequences

- Profile compilation validates correctness without acting as a change-approval
  system.
- Upstream and enrichment changes remain visible and reversible in Git.
- Profile authors edit ordinary typed values rather than patch operations.
- The canonical renderer input is independent of the upstream YAML schema.
- Git history records evidence and review without copying authoring provenance
  into every runtime profile.

## DD-023 — Embed and pin the default representative glyph source

**Status:** Accepted

### Context

Text previews must not change with fonts installed on the host or in a
container. Exact printer ROM glyphs are outside the initial fidelity contract,
but representative glyph rasterization must still be reproducible and
license-compatible.

### Decision

Bundle Noto Sans Mono 2.006 under the SIL Open Font License 1.1 and embed its
verified bytes in the Rust renderer. Rasterize with the pinned pure-Rust
`fontdue` implementation and a fixed one-bit alpha threshold.

Printer profiles remain authoritative for cell width, cell height, baseline,
spacing, and advancement. The source font's natural metrics never control
ESC/POS layout. Keep the glyph-provider boundary replaceable so a profile can
later select a canonical bitmap atlas or printer-specific glyphs.

### Consequences

- Rendering does not read fonts from the host system.
- Font, rasterizer, or threshold changes are deliberate rendering changes that
  require pixel-fixture review.
- The font asset retains its own license and hash alongside the project.
- Model-specific atlases can improve glyph fidelity without changing layout
  semantics.

## DD-024 — Own printer semantics and isolate standards-heavy symbol generation

**Status:** Accepted

### Context

Native barcode and two-dimensional-code commands combine two different
problems. ESC/POS defines state, command framing, printer capability, layout,
paper movement, and HRI behavior. The symbol standards define checksums,
compaction, error correction, masks, and logical bars or modules.

Implementing every symbol standard locally would give complete source control,
but source ownership alone does not improve correctness. Mature,
standards-focused implementations provide useful independent coverage of rules
that are easy to implement almost correctly.

### Decision

ESCPost owns ESC/POS parsing and every printer-visible symbol behavior,
including placement and scaling. It also owns the common one-dimensional
barcode encoders, whose algorithms are small enough to review against the
printer reference and barcode standards.

Use a replaceable internal adapter around a pure-Rust QR implementation to
produce an unscaled module matrix from raw bytes. Do not expose the dependency
through the public API and do not use its image-rendering features. The
renderer remains responsible for mapping modules to printer dots.

Treat a valid QR matrix as distinct from a firmware-identical QR matrix.
Segmentation and mask selection may differ between a standards-compliant
library and a particular printer firmware. Record that difference as a
documented divergence until hardware evidence requires a fork or replacement.

### Consequences

- Symbol libraries cannot move, scale, clip, or feed receipt content.
- The QR dependency can be audited, pinned, fuzzed, forked, or replaced behind
  one small boundary.
- One-dimensional behavior remains directly testable without a general image
  or barcode-rendering dependency.
- Exact module equality with a selected printer requires hardware fixtures;
  successful decoding alone is insufficient evidence.
- New symbol families may use the same dependency rule when outsourcing the
  standards-heavy portion materially improves correctness.

## DD-025 — Do not emulate incidental firmware quirks by default

**Status:** Accepted

### Context

Low-cost ESC/POS-compatible printers sometimes deviate from Epson behavior in
small, model-specific ways. Examples include thermal after-images, slightly
different barcode dimensions, unexpected HRI behavior, resident glyph shapes,
and different valid QR compaction or mask choices.

Replicating every observed difference would turn the renderer into a collection
of fragile firmware flags. Many observations are difficult to distinguish from
paper, heat, print-head, or configuration effects and do not prevent the PNG
from communicating the receipt's content and layout.

### Decision

Use documented ESC/POS behavior plus profile geometry and capabilities as the
default model. Do not emulate a firmware quirk merely because it creates a
pixel difference.

Add a model-specific behavior only when it is reproducible and materially
affects command parsing, content meaning, positioning, wrapping, feeds, cuts,
sheet boundaries, or another behavior needed by the product. Minor native
symbol size differences, HRI deviations, thermal artifacts, resident glyph
shapes, and standards-valid QR matrix differences may remain documented
divergences when the resulting receipt layout is still useful and correct.

Record observed but unmodeled quirks with the physical test case so the
decision can be revisited if the difference later matters.

### Consequences

- Printer profiles stay focused on behavior that materially improves previews.
- The renderer follows a reviewable protocol baseline instead of reverse
  engineering every firmware revision.
- Material geometry differences remain eligible for typed corrections. For
  example, the calibrated NT-5890K paints `ESC *` 8-dot rows adjacently instead
  of using Epson's three-dot vertical pitch, ignores negative `ESC \`, ignores
  `ESC $` after printable data, ignores `ESC J`, and feeds only the full-cut
  form of `GS V` Function B. It also consumes one LF immediately following
  `GS v 0`; this is modeled because it materially changes vertical placement.
- A preview may not reproduce every dot or native-symbol implementation choice
  even when its positions and overall layout are correct.
- A previously neglected quirk can become modeled after reproducible evidence
  and a concrete fidelity need justify the added complexity.

## DD-026 — Provide a virtual unrestricted reference profile

**Status:** Accepted

### Context

Physical printer profiles intentionally disable unsupported mechanisms and
capture firmware quirks. That makes them the wrong fixture for demonstrating
generic renderer behavior such as multiple cuts, modern graphics, or every
implemented barcode system.

Calling one physical model the ESC/POS default would also be misleading:
ESC/POS does not standardize paper width, DPI, resident font ROM, or cutter
placement.

### Decision

Maintain `REFERENCE` as a self-contained virtual profile. It imports no
upstream printer and explicitly enables every capability represented by the
current canonical profile schema. It follows documented baseline command
behavior without model-specific ignored-command rules. In the descriptor and
deviation model (DD-031), REFERENCE is the zero-deviation baseline: it states
virtual descriptors and enables every capability while turning on no deviations.

Give the virtual mechanism concrete, deterministic geometry so it can produce
PNG pixels. Treat those values as test parameters, not universal ESC/POS
dimensions. Keep parser and command coverage independent: REFERENCE removes
profile capability restrictions but does not make unimplemented commands
available.

### Consequences

- Generic demonstrations and golden cases do not need to falsify a physical
  printer profile.
- Adding a canonical capability requires an explicit REFERENCE decision.
- REFERENCE uses automated golden evidence and never claims physical
  verification.
- Integrations may use it for unrestricted previews when no target printer is
  known, while a real print preview should still select the actual device
  profile whenever possible.

## DD-027 — Keep the rendering crate free of I/O and platform dependencies

**Status:** Accepted

### Context

The `escpost-render` crate is embedded by the CLI and the Python binding today, and
the roadmap adds more consumers: replay, proxying, linting, and integration
into other projects. Some future hosts may not be developer-machine processes
at all; rendering inside a browser through WebAssembly is a realistic option
for the web viewer. Hardware access, networking, and terminal interaction
each narrow the set of environments the crate can run in and enlarge the
surface that must be audited and tested.

### Decision

The `escpost-render` crate performs pure computation: ESC/POS bytes and a profile
in, dot surfaces and PNG bytes out. It must not depend on networking,
hardware transports, filesystem access, system clocks, or any other
operating-system interface. Its dependency tree stays pure-Rust computation
(font rasterization, PNG encoding, symbol generation, text encodings), which
keeps the crate portable to any target Rust compiles to, including
WebAssembly.

Applications own I/O. USB and RAW TCP printing, the web server, and file
handling live in `escpost`. When a second consumer needs physical
output, extract the transports into a sibling crate such as `escpost-print`
instead of moving them into the renderer.

### Consequences

- Rendering stays deterministic and trivially testable: no renderer test
  needs a network, a device, or a clock.
- The renderer can be embedded in any host — PyO3 today, WebAssembly or
  other bindings later — without dragging transport dependencies along.
- New rendering features must express environmental needs as explicit inputs
  such as options, profiles, and byte streams instead of reaching for the
  operating system.
- Reusable physical printing becomes its own crate with its own dependency
  profile, extracted only when a second consumer exists.

## DD-028 — Condense representative glyphs to fill the profile cell

**Status:** Accepted

### Context

The bundled font (DD-023) is rasterized at `font_size = cell_height_dots` so
glyphs fill the cell vertically. Noto Sans Mono advances 0.6 em, which is 14.4
dots in the NT-5890K's 12-dot cell, so drawn one-to-one the wide glyphs overflow
their cell and print glued to their neighbours with no separating white space.

Three fixes were considered: clip the overflow (crops the sides off wide
glyphs), uniformly scale each glyph down (preserves proportions but shrinks the
text and leaves the cell underfilled vertically), or condense horizontally
(keeps full cell height, squeezes width to fit). A tempting fourth — measuring
each glyph and scaling it individually — was rejected outright: a per-glyph
scale factor makes stroke weights and proportions vary across a line and breaks
the monospace grid.

### Decision

Condense every glyph horizontally by a single font-wide factor so its advance
box coincides with the profile cell. The glyph then fills the cell in both axes
and retains its designed side bearings, so adjacent glyphs stop colliding
without reserving any artificial inter-glyph gutter.

The factor is one constant applied to every glyph, derived at runtime from the
font's own advance metric (`fontdue` advance width ÷ em, measured once and
memoized). It is never computed from an individual glyph's ink, and profiles
remain authoritative for cell size and advancement (consistent with DD-023).

The vertical axis is fitted the same way. `font_size` sets the em, but glyph ink
(descenders, accents) can extend past it, so a font can still overflow a cell
rasterized at its height — the bundled font's descenders are deeper than the ROM
font a profile was measured against, which clipped `g`/`y`/`p`. Two font-wide
measurements, taken once from the font, resolve this: the rasterization size is
reduced when the ink is taller than the em so the ink box matches the cell, and
the baseline is kept at the profile value unless descenders would still clip, in
which case it is lowered just enough to admit them. Both are uniform repositions
or scales — never per-glyph, and never a non-proportional squeeze of the
descender alone.

Condensing is area-correct: glyphs are rasterized at a small supersample of the
cell resolution and each output dot is inked from the mean coverage of its
samples, rather than picking one nearest source column. That keeps thin vertical
stems in dense glyphs (`W`, `M`, `m`) present and evenly weighted after the
horizontal squeeze. Output stays 1-bit on the profile's dot grid, so it remains
faithful and golden-comparable — the supersample only improves which dots the
condensed glyph lights.

The font provider and this whole family of derived measures (advance ratio, ink
extents, condense factor, fitted size, effective baseline, supersampled cell
mask) live in a dedicated `font` module. That module *is* the replaceable
glyph-provider boundary DD-023 calls for; glyph placement in `text.rs` only
consumes the resolved geometry and the returned ink mask.

Emphasized text (`ESC E`) stays a one-dot horizontal double-strike of the base
glyph, modeling the printer firmware's mechanism rather than swapping in a
separately designed bold weight, which would diverge from the dots the device
lays down.

Full-bleed glyphs whose ink spans the whole advance (`_`, `%`) therefore touch
consecutive copies of themselves. Physical printing on the NT-5890K confirms
this matches hardware: a run of underscores renders as one continuous rule, so
an earlier `cell_width - 1` gutter idea was rejected as it would insert gaps the
real printer does not produce.

### Consequences

- Wide glyphs are no longer cropped, normal text no longer prints glued, and
  descenders are no longer clipped.
- Replacing the bundled font needs no code change: every factor — horizontal and
  vertical — recomputes itself from whatever font is embedded.
- The change is a deliberate rendering change, so its golden PNG fixtures were
  re-blessed after visual review (DD-023's pixel-fixture rule).
- A profile that later selects a printer-specific bitmap atlas can bypass this
  mapping entirely, since the `font` module keeps the glyph-provider boundary
  replaceable.

## DD-029 — Anti-aliased grayscale previews, kept distinct from the faithful dots

**Status:** Accepted

### Context

The 1-bit dot grid is faithful to what a thermal printer prints, but on screen
its hard-edged glyphs read as harsh — a preview-oriented viewer (`serve`, the
web viewer) benefits from smoother text. A printer cannot lay down gray, so any
smoothing is cosmetic and must never be mistaken for real output or leak into
the golden comparison.

### Decision

The surface stores 8-bit coverage per subpixel at `scale ×` the dot resolution,
governed by two independent options:

- `scale` — pixel density (1 to 3 subpixels per dot).
- `antialias` — encoding. When off, glyph coverage is thresholded to hard dots
  and the sheet packs to a 1-bit PNG; the values are 0/255 so the faithful path
  is bit-identical to bit-packing and the golden fixtures never move. When on,
  glyph edges keep their coverage and the sheet encodes as 8-bit grayscale.

Keeping the two orthogonal — rather than deriving anti-aliasing from `scale > 1`
— removes a hidden coupling and collapses the renderer to a single glyph blit:
only the final threshold (or not) and the encoder differ. Dot-native content
(barcodes, `GS v0` bitmaps, reverse fills, underlines) always fills hard blocks,
since only vector glyphs benefit from smoothing; smoothing a barcode would
misrepresent it. Reverse video carves glyph coverage out of the ink block, size
multipliers replicate each subpixel, and emphasis smears one dot — all in the
one blit.

The library defaults to `scale = 1`, `antialias = false` (faithful), so
`render()` and every golden test are unaffected. The CLI exposes `--scale <N>`
for values 1 through 3 and `--antialias[=<bool>]`: `render` defaults to faithful
(`1`, off) so its artifacts stay true; `serve` defaults to a `3 ×` grayscale
preview (nicer out of the box), and either can be overridden within that range.

### Consequences

- Previews look markedly smoother without changing what the renderer claims the
  printer prints; the faithful path and its goldens are byte-for-byte unchanged.
- Coverage storage costs ~8× the memory of bit-packing on the faithful path —
  acceptable for receipt-sized sheets, and the simplification is worth it.
- The grayscale sheet is a cosmetic presentation output; it is never compared
  against goldens and carries no fidelity guarantee.

## DD-030 — Render every cut as a receipt boundary, warning when the paper cannot be cut

**Status:** Accepted

### Context

A cut command (`GS V 0`/`1`, or Epson Function B `GS V 65`/`66 n`) carries two
meanings: a boundary between receipts, and a physical severing of the paper.
Many supported printers — the NT-5890K among them — have no cutter and tear at a
manual bar instead, so they honour the first meaning and ignore the second. A
render must not fail over a cut the printer itself would simply feed past.

### Decision

An acted-upon cut always marks a receipt boundary and splits the preview into
separate sheets, whether or not the printer can physically cut. When the profile
has a matching cutter the split is silent. When it does not, the render still
succeeds and records a non-fatal `RenderWarning::UncuttableCut` (carrying the
command, profile, and byte offset) so callers can report that the paper was not
severed.

`RenderResult` carries a `warnings: Vec<RenderWarning>` channel alongside
`device_events` — for diagnostics that do not fail a render but note where the
preview diverges from the printer's physical behaviour.

"Acted-upon" is the qualifier: a firmware quirk that discards a cut outright (the
NT-5890K ignores Function B `GS V 66`) is a true no-op — no split, no warning —
because the printer never acts on it. Only cuts the firmware performs (Function A,
and fed Function B) split and warn.

### Consequences

- A cutter-less profile previews multi-receipt jobs correctly, and no input a
  physical printer would tolerate fails to render.
- Callers choose how loud to be: the `render` CLI prints warnings to stderr, the
  `serve` viewer shows an amber non-fatal notice, and the Python binding exposes
  a `warnings` list — the render itself is unaffected.
- The split is a preview convenience, not a claim that the paper was cut; the
  warning is what distinguishes the two.

## DD-031 — Model profile parameters as descriptors and deviations

**Status:** Accepted

### Context

A printer profile mixes two unlike things: intrinsic physical facts, and firmware
behaviors. ESC/POS documents a baseline for the behaviors but standardizes none
of the physical facts — DD-026 notes it defines no paper width, DPI, resident
font ROM, or cutter placement. Treating every field as a bare value hides that
distinction; treating every field as a "deviation from REFERENCE" would falsely
elevate REFERENCE's virtual dimensions into a standard that printers depart from.

### Decision

Split profile parameters into two kinds.

**Descriptors** are intrinsic physical facts with no spec norm: printable width,
horizontal and vertical DPI, motion units, font cell metrics, cutter distance,
capabilities, and the available code-page map. They are sourced from the shared
upstream database (DD-018) or measured from the device.

**Deviations** are departures from the documented ESC/POS baseline: the command
behaviors (`ESC \` negative positioning, `ESC $` after printable data, `ESC J`,
the LF following `GS v 0`, and `GS V` Function B full and partial), the `ESC *`
8-dot vertical pitch, carriage-return handling, and the power-on defaults (line
spacing, active code-page slot, international character set). Each has a
conformant baseline; a profile turns on only the departures it has confirmed.

Both kinds share one axis. An omitted parameter is **assumed** — it takes its
default value, or stays conformant. A stated parameter is **known** — a measured
or sourced value, or a confirmed deviation. Stating a parameter is itself the
confirmation. There is no separate provenance level and no per-field disclosure
record.

REFERENCE (DD-026) is the zero-deviation baseline: it states virtual descriptors
and enables every capability, but turns on no deviations. A physical profile
states the descriptors it knows and turns on the deviations it has verified.

The base an omitted parameter falls back to is a set of default values, not an
ancestor profile. ESCPost has no profile inheritance, so a profile stays flat,
local, and content-addressed (DD-006), unlike upstream's `inherits:` graph, which
is flattened at build time.

Renderer-wide fidelity limits — representative glyphs (DD-007, DD-023), QR mask
choice (DD-024), unmodeled thermal artifacts (DD-025) — are not profile
parameters. They belong to the standing fidelity contract, and an observed but
unmodeled per-printer quirk is recorded with its physical test case (DD-025).

### Consequences

- Authoring or calibrating a printer means stating the descriptors you know and
  toggling the deviations you confirm; everything unstated is visibly assumed by
  its absence, so the profile is its own calibration checklist.
- The canonical profile stores resolved values; whether a profile rests on
  assumed defaults or on calibration is signaled at the profile level by its
  source (DD-032), not by a per-field record.
- No confidence prose or disclosure list is maintained: render honesty comes from
  the standing fidelity contract plus the profile-level source.
- The command quirks a profile turns on are exactly the model-specific behaviors
  DD-025 admits, expressed as explicit deviations rather than ad hoc corrections.

## DD-032 — Make upstream printers available by synthesizing against the default base

**Status:** Accepted

### Context

The shared upstream database (DD-018) provides descriptors — capabilities, code
pages, media width and DPI, font columns — for many printers, but not the full
descriptor and deviation set an emulator needs. Requiring a complete typed
enrichment (DD-022) before a printer is usable means an upstream identifier
either carries a hand-authored enrichment or has no runtime profile at all, which
blocks the ordinary act of adopting a printer and calibrating it incrementally.

### Decision

Every profile parameter is optional; an omitted parameter takes the default base
(DD-031). Synthesize a renderable profile for every upstream entry whose
printable width is derivable — from the upstream media pixels or an enrichment —
filling the remaining descriptors from upstream where present (DPI, font cell
width from width divided by columns, capabilities, code pages) and otherwise from
documented constants, and leaving every deviation conformant unless an enrichment
turns it on.

The default capability posture is conservative: nothing is claimed until stated,
so an uncalibrated profile never over-advertises. The default paper width is the
smaller common thermal size (58 mm), which fails safe by wrapping content early
rather than overrunning a narrower sheet. Width is never fabricated for a
real-named printer: an upstream entry that states no width — the generic
`default`, `safe`, and `simple` templates — produces no profile and is logged. A
human-authored profile may still omit width and accept the 58 mm default, because
a person then owns that choice.

A synthesized profile carries a distinct source marker, separate from a curated
enrichment, so an assumed profile never presents as a physically reviewed one;
that marker is the runtime signal that a profile rests on base defaults rather
than calibration.

All profiles resolve at build time into the single canonical pack (DD-018).
Git records changes to the source inputs and generated output; compilation does
not add a separate approval gate.

### Consequences

- Shared upstream identifiers resolve without a hand-authored enrichment per
  printer; adoption starts from nothing and grows as deviations are confirmed and
  descriptors measured.
- An uncalibrated profile cannot over-claim capabilities or overrun a wider sheet
  than assumed.
- A generic template with no width yields no profile; REFERENCE (DD-026) remains
  the choice when no specific printer is known.
- The 58 mm default width and the constant descriptor defaults are heuristics
  open to revision as calibrated evidence accumulates.
- Runtime resolution stays a pack lookup; the larger pack must be regenerated on
  upstream or default changes, enforced by the drift check.

## DD-033 — Keep conceptual trace sheets and buffered output distinct from rendered sheets

**Status:** Accepted

### Context

In Standard mode, some ESC/POS input writes image data into the current print
buffer without printing it immediately. Printable characters and `ESC *` column
images are examples. A later print operation such as `LF` prints that buffered
line; if the job ends first, the printer still has buffered data but no
corresponding marks exist on paper.

The renderer already reflects this distinction internally. `PrinterState`
creates an active roll and line buffer at initialization, but returns a
`RenderedSheet` only after committed output gives the roll a height or a cut
finalizes it. Deriving trace sheets solely from returned render surfaces would
therefore discard successfully parsed commands whenever all of their paint
remains buffered. Automatically flushing at end of input would instead invent
printer behavior and falsely show unprinted data in the PNG.

A whole sheet also cannot simply be called uncommitted: one receipt may contain
many committed lines followed by one final buffered line.

### Decision

Tracing has a **conceptual active sheet** independently of whether that sheet
has a rendered PNG. Sheet zero exists internally from renderer initialization;
it is emitted into the trace once the job records a command. A cut closes the
current conceptual sheet and establishes the next one. Rendered sheets remain
authoritative paper output and are never fabricated merely to host trace data.

Each successfully parsed command retains its command record. Paint-producing
commands additionally distinguish the disposition of their output:

- **Buffered** — logical output exists only in the current print buffer and is
  not visible on a rendered sheet.
- **Committed** — output reached the roll and may carry final sheet-space paint
  bounds.
- Commands with no paint output have no paint disposition; state, motion, device
  action, and as-yet-unmodeled effects remain distinct concepts.

A print operation transitions the affected earlier commands from Buffered to
Committed. The trace may retain the committing command as the cause of that
transition, so a consumer can explain that `LF`, `CR`, `ESC J`, `ESC d`, or
another modeled print operation caused buffered content to reach paper.
Commands that print immediately enter the Committed state directly.

At end of input, remaining buffered commands stay Buffered. They appear in the
command stream with an explicit “not printed” status, receive no overlay on the
authoritative PNG, and do not cause an implicit line feed. A trace sheet may
therefore exist without a corresponding rendered sheet, or may contain both
committed and buffered commands while referencing one rendered sheet.

### Consequences

- Every successfully parsed command remains inspectable even when the job
  produces no PNG.
- The web workbench can explain missing output instead of silently omitting a
  command or pretending buffered pixels were printed.
- Commitment is tracked per paint-producing command, not as one coarse sheet
  flag.
- Trace-sheet and rendered-sheet counts are no longer required to match. Their
  ordered indexes correspond when rendered output exists; a final zero-height
  conceptual sheet may have no rendered entry.
- Final sheet-space bounds exist only for committed paint. A future buffered
  preview may expose line-local logical bounds, but it must remain visually and
  semantically separate from printed output.
- Ordinary non-traced rendering keeps its current end-of-input behavior.

## Open questions

The following are intentionally not decided yet:

- canonical runtime profile fields for full command coverage and their
  compatibility policy;
- whether bidirectional status commands need a configurable response emulator.
