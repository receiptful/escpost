# Command tracing

## Purpose

Command tracing explains how an immutable ESC/POS byte stream changes printer
state and produces rendered output. A PNG answers what the simulated printer
produced; a trace connects that output to the exact input commands that caused
it.

Tracing is optional. Ordinary `render` calls continue to use the plain
monochrome surface and do not allocate trace records, copy command bytes, or
calculate trace-only state. Callers opt in through the experimental
`render_with_trace` or `render_with_trace_and_options` API. The returned Rust
types are public so applications can explore the feature, but their shape is
not yet stable.

## Rendering surfaces

The renderer is generic over the private `RenderSurface` contract:

```text
PrinterState<S: RenderSurface>
                │
                ├── MonoSurface
                │     canonical raster and PNG output
                │
                └── TracingSurface
                      wraps MonoSurface for traced renders
```

The implementation is split by responsibility:

```text
crates/escpost-render/src/surface/
├── mod.rs       RenderSurface contract and module exports
├── mono.rs      monochrome raster storage and PNG encoding
└── tracing.rs   provenance-decorating surface used by traced renders
```

`TracingSurface` forwards every drawing operation to its inner surface. This
keeps traced and untraced rendering on the same command interpreter and raster
implementation. Static generic dispatch lets an optimized ordinary render
compile away the default no-op command hook.

`RenderSurface::fork` creates an empty related surface while preserving
decorator context. The renderer uses it for temporary line buffers, resized
print areas, HRI text, and sheets created after cuts. Trace metadata therefore
follows the same composition and positioning operations as pixels.

## Command decoding seam

The current proof also has a private, compile-time `CommandSink` seam. A traced
render specializes the interpreter with a recording sink; an ordinary render
specializes it with `NoTrace`:

```text
render_surfaces_with_sink<S, C: CommandSink>
                              │
                              ├── C::ENABLED = false → NoTrace
                              └── C::ENABLED = true  → recording sink
```

The interpreter always decodes the parameters needed to execute a command.
It constructs the additional semantic `DecodedCommand` value and byte-range
record only inside an `if C::ENABLED` branch. Thus the ordinary path does not
construct decoded-command objects, call the surface command hook, allocate
trace storage, copy payloads, format values, use dynamic dispatch, or test a
runtime tracing flag. Static generic dispatch lets an optimized build remove
the disabled branch entirely.

This is a source-level performance contract, backed by the command-heavy
release benchmark in `examples/render_bench.rs`. Exact machine-code identity
is a compiler outcome rather than a Rust language guarantee, so benchmark
comparisons remain part of changes to this seam.

The vertical slice currently models justification, printable bytes, line feed,
`GS v 0` raster images, and QR print operations. Every other successfully
parsed command receives an unmodeled fallback identity with no fabricated
effects. The slice validates the abstraction, its disabled-path cost, and an
end-to-end consumer before the complete command model is designed.

## Experimental public API

`render_with_trace` mirrors `render`, and `render_with_trace_and_options`
mirrors `render_with_options`. Both return `TracedRenderResult`, which contains
the ordinary `RenderResult` plus a `Trace`:

```rust
let result = escpost_render::render_with_trace(data, &profile)?;

for sheet in result.trace.sheets {
    for command in sheet.commands {
        // Inspect its byte range, decoded command, and typed effects.
    }
}
```

The experimental trace types deliberately do not implement Serde. This keeps
the renderer's in-memory model independent from any persistent or wire format.
The CLI maps those types into its own web-specific JSON data transfer objects;
that JSON is also experimental and is not a stable serialization contract.

The trace groups commands by conceptual output sheet. `Trace::sheets[n]` and
`RenderResult::sheets[n]` refer to the same ordered sheet when rendered output
exists. A final conceptual sheet containing only buffered output has no
corresponding rendered entry or PNG. A command belongs to the conceptual sheet
that was active when the command began executing. For now, one command is
assumed to affect at most one sheet.

## Target production model

The following sections specify intended production behavior. The current
implementation produces typed entries for five command types and generic
entries for every other successfully parsed command; its narrower guarantees
are listed under
[Current vertical slice](#current-vertical-slice).

### Command identity

Every safely framed command will be identified by its byte range in the
submitted input. The range, not a copied payload, is the authoritative link
back to the immutable source. A future serialized trace may include raw bytes
for convenience, but they must match that range exactly.

The current vertical slice records the complete range after parsing determines
the command length and uses its starting offset to attribute logical bounds.
The QR print entry additionally carries the effective stored QR payload so a
consumer can label the symbol; this is derived state, not a replacement for the
print command's authoritative input range.

Printable bytes may initially appear as individual commands. Grouping adjacent
text bytes into display runs is a presentation decision and must not lose the
underlying byte boundaries.

### Command effects

Every parsed command will receive a trace entry, including commands that paint
nothing. A command can have more than one effect:

- **Paint** — logical drawing bounds on the command's output sheet.
- **State change** — typed before/after values such as justification, font,
  print area, or line spacing.
- **Motion** — logical print-position movement, including the positions before
  and after the command.
- **Device event** — a drawer pulse or another non-printing physical action.
- **Sheet boundary** — a completed sheet and the cut that caused it.
- **Ignored** — a valid command that had no effect, with a typed reason.
- **Diagnostic** — malformed, unsupported, unavailable, clipped, or otherwise
  noteworthy behavior.

This model avoids inventing a painted rectangle for state-only commands. The
CLI and web interface can visualize each effect appropriately: a region, state
diff, movement marker, event, boundary, or diagnostic.

### Logical drawing bounds

The tracer will store one logical bounding rectangle for a painting command,
not individual contributed pixels. For text, this is the complete character
cell, including blank space and character spacing, rather than the tight bounds
of its visible ink. A space therefore has highlightable bounds even though it
does not change the raster.

The surface decorator receives an explicit logical-region marker from the
renderer. It carries that rectangle through buffering, justification, and
composition into final sheet coordinates without observing individual raster
writes. Trace finalization unions multiple logical markers from the same
command into a single bounding rectangle.

Because commands are grouped under `SheetTrace`, the rectangle does not carry a
sheet index. The experimental paint effect has the shape
`Effect::Paint { bounds }` rather than a list of sheet-indexed regions.

Final visible pixel ownership is not stored initially. Later commands may
overlap, reverse, or erase earlier output, making a single final owner
ambiguous. If the UI eventually needs exact final-visible selection, it should
derive that view from command effects under separately documented overlap
rules.

The current implementation records explicit logical bounds and never observes
or stores individual raster writes for tracing.

### Buffered output and motion

Standard-mode text and column graphics are first painted into a line-local
surface. Their final sheet position is not known until a feed operation applies
the print area and justification.

Internally, when `LF` commits a line:

1. bounds already belong to the commands that produced the buffered content;
2. composition translates those bounds into final sheet coordinates;
3. `LF` records its own print-position movement.

Each paint-producing `CommandTrace` also carries a `PaintLifecycle`.
Line-buffered text and column graphics begin as `Buffered`; finalization
promotes them to `Committed` when their regions are present on a rendered roll.
Commands such as raster images and QR print operations that draw directly onto
the roll begin as `Committed`. Commands without logical paint have no paint
lifecycle.

`LF` does not take ownership of the committed pixels. In the web interface,
hovering the printable command highlights its final rectangle, while hovering
`LF` can show before/after position markers and a paper-advance indicator.

The trace does not currently expose a relationship between the command that
created buffered content and the command that committed it. Exact commit timing
can depend on printer firmware, buffer capacity, configuration, and documented
profile deviations. Add such a relationship only when a user-facing need and
the profile model can give it defensible semantics.

The same rule applies to other positioning and feed commands: they record
motion rather than fabricated paint.

### State, resources, and events

State-setting commands will record only values they actually change. An ignored
setting records an `Ignored` effect and its reason rather than a false state
transition.

Commands that store QR or graphics data change an internal resource but paint
nothing. The later print command owns the resulting painted region and may
reference the stored-data command as an input dependency.

Cuts can combine motion and a sheet boundary. Drawer pulses and similar
non-printing actions are device events. Initialization records restored state
and any buffered data it discards.

### Errors and safe framing

Tracing should eventually remain useful when strict rendering fails. It will
record every command whose boundary and effects were established safely, then
report the failure at the exact byte offset. Remaining bytes may be exposed as
opaque input but must not be presented as speculatively decoded commands.

The current API returns `RenderError` without a partial trace when rendering
fails.

Diagnostics must distinguish malformed or truncated input, an unimplemented
valid command, a profile-unavailable command, an ignored command, clipped
output, and a profile-confirmed behavioral deviation.

## Current vertical slice

The tracer assembles a public experimental `Trace` containing ordered
conceptual `SheetTrace` values, including a sheet whose paint remains entirely
buffered and therefore has no rendered PNG. Each `CommandTrace` has its exact
input byte range, a semantic `DecodedCommand`, an optional paint lifecycle, and
typed effects. The slice implements
justification state changes, printer-position motion, and one logical
`PaintRegion` bound for printable bytes, `GS v 0` raster images, and QR print
operations. Image bounds cover their complete logical drawing area rather than
only their dark pixels. `ESC a` and `LF` receive their respective state-change
and motion effects without fabricated paint. Unmodeled commands retain their
control/`ESC`/`GS` family and opcode with an empty effect list.

The end-to-end test verifies that:

- traced and ordinary raster surfaces are identical;
- `ESC a` records the `Left` to `Center` state transition without paint;
- text and space cells retain their printable byte's input range and are
  translated into their final position on the active sheet;
- commands before and after a cut are grouped under their respective sheets;
- `LF` records its before/after position without taking ownership of the text
  command's paint;
- raster images retain their full logical dimensions even when most pixels are
  blank;
- QR storage receives an unmodeled fallback entry while the print operation
  owns the symbol bounds and exposes its effective stored payload;
- unmodeled commands retain exact ranges without fabricated effects;
- un-fed paint remains Buffered without receiving final sheet-space bounds or
  causing an implicit end-of-input feed; and
- line feeds promote buffered paint to Committed, while immediate-print
  commands enter the Committed state directly.

The slice does not yet provide typed identities and effects for other commands,
return a partial trace on failure, or make its in-memory representation a
stable public contract.

## Web workbench

When the CLI web mode is active, it uses the traced renderer and exposes every
successfully parsed command through `/api/jobs/current`. Five command types have
specialized presentation; the rest use their protocol family and opcode with a
default “annotations not yet modeled” description. Non-web CLI rendering
continues to call the ordinary renderer.

The workbench shows a command list beside the authoritative PNG receipt. Each
receipt image has an SVG overlay in the same printer-dot coordinate system.
Hovering or focusing a command highlights its logical drawing bounds; hovering
the bound previews the corresponding command; clicking either side pins or
unpins the selection. State-only and motion-only commands remain selectable in
the list but have no fabricated painted rectangle. A command group whose paint
remains buffered at end of input is labeled “Not printed”; it remains in the
list even when its conceptual sheet has no PNG. QR bounds carry a badge on
their bottom edge containing a display-safe form of the encoded payload.
Activating the badge copies the exact text payload; an `http://` or `https://`
payload also opens in a new tab. The QR command item repeats the payload as a
link when applicable and always provides a separate copy control. On narrow
screens the receipt appears before the command list.

## Open design decisions

Before declaring tracing stable, decide and document:

- the Rust trace types and versioned JSON representation;
- typed state deltas and command parameter models;
- logical-bound clipping and union rules;
- how command dependencies such as stored data and later printing are linked;
- trace behavior on render errors and resource-limit failures;
- whether traces are retained only in memory or persisted with captures; and
- whether and how the experimental web JSON becomes a versioned format.

These decisions belong to the trace model, not to `MonoSurface` or the ordinary
PNG rendering API.
