import { copyText, webUrl } from "./annotation";
import { STICKY_HEADER } from "./reveal";
import type preact from "preact";
import type { StyleDefaults, TextStyle } from "../../api/types";
import type { CommandGroup, CommandGroupView } from "./model";

type Props = {
  groups: CommandGroup[];
  /** How many bytes the job holds, which the panel explains one by one. */
  byteCount: number;
  /** The style the printer profile starts the job with. */
  styleDefaults: StyleDefaults;
  /** Where the bytes of the job can be had, while the job is whole. */
  inputUrl?: string;
  previewedGroupId: string | null;
  pinnedGroupId: string | null;
  previewedCharacter: number | null;
  onPreviewCharacter: (index: number) => void;
  onPreviewCharacterEnd: () => void;
  panelRef: (element: HTMLElement | null) => void;
  register: (id: string, element: HTMLElement | null) => void;
  onPreview: (id: string) => void;
  onPreviewEnd: (id: string) => void;
  onPin: (id: string) => void;
};

/** One style of the toolbar, marked while the printer holds it.
 *
 * A style that is off keeps its place, so a reader sees the whole set at once
 * and finds each style where it stood on the row above. */
function Chip({ label, active, children, joined }: {
  /** The whole label, because only some styles read as on or off. */
  label: string;
  active: boolean;
  children: preact.ComponentChildren;
  joined?: boolean;
}) {
  return (
    <span
      aria-label={label}
      title={label}
      data-active={String(active)}
      class={`px-1.5 py-0.5 text-[0.7rem] leading-none ${
        joined
          ? "border-y border-r first:rounded-l-sm first:border-l last:rounded-r-sm"
          : "rounded-sm border"
      } ${
        active
          ? "border-base-content/60 bg-base-content/25 text-base-content"
          : "border-dashed border-base-content/20 text-base-content/30"
      }`}
    >
      {children}
    </span>
  );
}

/** Shows the style a command printed with, the way a word processor shows the
 * styles of the text under the caret: every style listed, the ones in force
 * marked and the rest dimmed. */
function TextStyleBar({ textStyle, styleDefaults }: {
  textStyle: TextStyle;
  styleDefaults: StyleDefaults;
}) {
  // A style the printer profile decides counts as set only where a command
  // moved it away from what the profile starts with.
  const toggle = (on: boolean) => on ? "on" : "off";
  const chosen = (on: boolean) => on ? "selected" : "not selected";
  const orDefault = (set: boolean) => set ? "" : " (default)";
  const underline = textStyle.underline_thickness > 0;
  const codePage = `${textStyle.encoding ?? `page ${textStyle.code_page}`}, ${textStyle.international_character_set}`;
  const codePageSet = textStyle.code_page !== styleDefaults.code_page
    || textStyle.international_character_set !== styleDefaults.international_character_set;
  const spacingSet = textStyle.line_spacing_dots !== styleDefaults.line_spacing_dots;
  return (
    <span aria-label="Text style" class="mt-2 flex flex-wrap items-center gap-1.5 font-mono">
      <span class="flex">
        <Chip label={`Font A: ${chosen(textStyle.font === "A")}`} active={textStyle.font === "A"} joined>
          A
        </Chip>
        <Chip label={`Font B: ${chosen(textStyle.font === "B")}`} active={textStyle.font === "B"} joined>
          B
        </Chip>
      </span>
      <Chip label={`Bold (Emphasized): ${toggle(textStyle.emphasized)}`} active={textStyle.emphasized}>
        <span class="font-bold">B</span>
      </Chip>
      <Chip
        label={underline
          ? `Underline: on, ${textStyle.underline_thickness} dot`
          : "Underline: off"}
        active={underline}
      >
        <span class="underline underline-offset-2">U</span>
      </Chip>
      <Chip
        label={`White on black (Reverse): ${toggle(textStyle.reversed)}`}
        active={textStyle.reversed}
      >
        ◧
      </Chip>
      <span class="flex">
        <Chip
          label={`Align left: ${chosen(textStyle.justification === "left")}`}
          active={textStyle.justification === "left"}
          joined
        >
          ⇤
        </Chip>
        <Chip
          label={`Align centre: ${chosen(textStyle.justification === "center")}`}
          active={textStyle.justification === "center"}
          joined
        >
          ≡
        </Chip>
        <Chip
          label={`Align right: ${chosen(textStyle.justification === "right")}`}
          active={textStyle.justification === "right"}
          joined
        >
          ⇥
        </Chip>
      </span>
      <Chip
        label={`Character width: x${textStyle.width_magnification}${orDefault(textStyle.width_magnification > 1)}`}
        active={textStyle.width_magnification > 1}
      >
        {`${textStyle.width_magnification}xW`}
      </Chip>
      <Chip
        label={`Character height: x${textStyle.height_magnification}${orDefault(textStyle.height_magnification > 1)}`}
        active={textStyle.height_magnification > 1}
      >
        {`${textStyle.height_magnification}xH`}
      </Chip>
      <Chip label={`Code page: ${codePage}${orDefault(codePageSet)}`} active={codePageSet}>
        {textStyle.encoding ?? `page ${textStyle.code_page}`}
      </Chip>
      <Chip
        label={`Line spacing: ${textStyle.line_spacing_dots} dots${orDefault(spacingSet)}`}
        active={spacingSet}
      >
        {`↕${textStyle.line_spacing_dots}`}
      </Chip>
      <Chip
        label={`Character spacing: ${textStyle.right_side_character_spacing_dots} dots${
          orDefault(textStyle.right_side_character_spacing_dots > 0)
        }`}
        active={textStyle.right_side_character_spacing_dots > 0}
      >
        {`↔${textStyle.right_side_character_spacing_dots}`}
      </Chip>
    </span>
  );
}

/** Tells whether the parameters are short and fixed enough to sit inline. */
function inlineParameters(view: CommandGroupView): boolean {
  return view.fixedParameters && view.parameterBytes.length > 0;
}

/** Shows each parameter byte on its own, over the character it printed.
 *
 * A byte keeps a fixed width, thus marking one of them never moves the rest. */
function ParameterBytes({ view, active, onPreview, onPreviewEnd }: {
  view: CommandGroupView;
  active: number | null;
  onPreview: (index: number) => void;
  onPreviewEnd: () => void;
}) {
  const pairing = view.characterPairing;
  return (
    <>
      {view.parameterBytes.map((byte, index) => (
        <span
          key={index}
          data-byte={index}
          class={`flex w-[2.5ch] flex-col items-center rounded-sm leading-tight ${
            // A shade on every second byte keeps one cell apart from the next.
            index % 2 === 1 ? "bg-base-content/5" : ""
          } ${pairing && active === index ? "font-bold ring-1 ring-base-content/40" : ""}`}
          onPointerEnter={pairing ? () => onPreview(index) : undefined}
          onPointerLeave={pairing ? onPreviewEnd : undefined}
        >
          <span data-hex={index}>{byte.hex}</span>
          {pairing && (
            // A printed space carries no ink, but it holds a place on the
            // paper. `whitespace-pre` keeps it, and a byte that names no
            // character borrows the same space to hold its line open.
            <span data-character={index} class="whitespace-pre text-base-content">
              {byte.character === "" ? " " : byte.character}
            </span>
          )}
        </span>
      ))}
      {view.parameterBytes.length < view.totalParameterBytes && (
        <span class="text-base-content/60">… ({view.totalParameterBytes} bytes)</span>
      )}
    </>
  );
}

export function CommandPanel(props: Props) {
  return (
    <aside
      ref={props.panelRef}
      aria-label="ESC/POS bytes in the current print job"
      // The panel and the sheets are cells of one row of the grid, thus the
      // grid gives them the same top and the same height on its own. The panel
      // asks for no height of its own, which would only stand against that.
      class="flex max-h-[70vh] flex-col overflow-auto rounded-box border border-base-300 bg-base-100 xl:max-h-none xl:min-h-0"
    >
      {/* The rows pass under the header, thus it needs a surface and a shadow
          of its own to stay apart from the row that scrolls behind it. */}
      <div
        {...{ [STICKY_HEADER]: "" }}
        class="sticky top-0 z-10 border-b border-base-content/10 bg-base-300 p-4 shadow-sm"
      >
        <div class="flex items-center gap-2">
          <h2 class="flex items-center gap-2 text-lg font-bold">
            {props.byteCount} bytes
            <span class="badge badge-outline badge-sm font-normal">ESC/POS</span>
          </h2>
          {/* The bytes of the job are had where the job names them, thus the
              heading needs no more than "Download" to say what it downloads. */}
          {props.inputUrl && (
            <a class="btn btn-ghost btn-xs ml-auto" href={props.inputUrl} download>Download</a>
          )}
        </div>
        {/* The rows below carry the same two columns. A reader of a row hears
            its own label, thus these headings serve the eye alone. */}
        <div
          aria-hidden="true"
          class="mt-2 flex items-baseline justify-between text-xs text-base-content/60"
        >
          <span>Command</span>
          <span>Index</span>
        </div>
      </div>
      <ol class="divide-y divide-base-300">
        {props.groups.map((group, row) => {
          const view = group.view;
          const previewed = props.previewedGroupId === group.id;
          const pinned = props.pinnedGroupId === group.id;
          const href = view.annotation ? webUrl(view.annotation.content) : null;
          return (
            <li
              key={group.id}
              // A shade on every second row keeps neighbouring commands apart.
              // It covers the whole row, thus the hover and pinned colours of
              // the command stay the only inset ones.
              class={`p-2 ${row % 2 === 1 ? "bg-base-200/40" : ""}`}
            >
              <button
                ref={(element) => props.register(group.id, element)}
                type="button"
                aria-label={`${view.name} ${view.firstByte}..${view.lastByte}: ${view.detail}`}
                aria-pressed={pinned}
                class={`w-full rounded-lg border p-3 text-left transition-colors ${
                  pinned
                    ? "border-info bg-info/15"
                    : previewed
                      ? "border-base-content/30 bg-base-200"
                      : "border-transparent hover:bg-base-200"
                }`}
                onPointerEnter={() => props.onPreview(group.id)}
                onPointerLeave={() => props.onPreviewEnd(group.id)}
                onFocus={() => props.onPreview(group.id)}
                onBlur={() => props.onPreviewEnd(group.id)}
                onClick={() => props.onPin(group.id)}
              >
                <span class="flex items-baseline justify-between gap-3">
                  <span class="flex min-w-0 flex-wrap items-baseline gap-1.5 font-mono text-xs">
                    <span class="text-sm font-bold">{view.name}</span>
                    {view.codeBytes && (
                      <span
                        aria-label="Command bytes"
                        class="rounded border border-base-content/30 px-1.5 py-0.5 font-bold"
                      >
                        {view.codeBytes}
                      </span>
                    )}
                    {inlineParameters(view) && (
                      <span
                        aria-label="Parameter bytes"
                        class="flex flex-wrap items-center gap-x-1 gap-y-1 rounded border border-base-content/20 px-1.5 py-0.5 text-base-content/70"
                      >
                        <ParameterBytes
                          view={view}
                          active={previewed ? props.previewedCharacter : null}
                          onPreview={(index) => {
                            props.onPreview(group.id);
                            props.onPreviewCharacter(index);
                          }}
                          onPreviewEnd={props.onPreviewCharacterEnd}
                        />
                      </span>
                    )}
                  </span>
                  <span class="font-mono text-xs text-base-content/55">{view.firstByte}..{view.lastByte}</span>
                </span>
                {/* A run of characters carries its text over its bytes, thus
                    it needs no line of its own for the same text. */}
                {!view.characterPairing && (
                  <span class="mt-1 block break-words text-sm">{view.detail}</span>
                )}
                {!inlineParameters(view) && view.parameterBytes.length > 0 && (
                  <span class="mt-2 flex font-mono text-xs">
                    <span
                      aria-label="Parameter bytes"
                      // A cell stands two lines tall, thus wrapped lines of a
                      // run need more room between them than the bytes need
                      // beside each other.
                      class="flex min-w-0 flex-wrap items-center gap-x-1 gap-y-2.5 rounded border border-base-content/20 px-1.5 py-1 text-base-content/70"
                    >
                      <ParameterBytes
                        view={view}
                        active={previewed ? props.previewedCharacter : null}
                        onPreview={(index) => {
                          props.onPreview(group.id);
                          props.onPreviewCharacter(index);
                        }}
                        onPreviewEnd={props.onPreviewCharacterEnd}
                      />
                    </span>
                  </span>
                )}
                {view.textStyle && view.showsStyle && (
                  <TextStyleBar textStyle={view.textStyle} styleDefaults={props.styleDefaults} />
                )}
                {view.paintLifecycle === "buffered" && (
                  <span class="badge badge-warning badge-sm mt-2">Not printed</span>
                )}
              </button>
              {view.annotation && (previewed || pinned) && (
                <div class="mt-2 flex items-center gap-2 px-2 text-sm">
                  {href ? (
                    <a
                      class="link link-primary min-w-0 flex-1 truncate"
                      href={href}
                      target="_blank"
                      rel="noopener noreferrer"
                      onClick={() => void copyText(view.annotation?.content ?? "")}
                    >
                      {view.annotation.label}
                    </a>
                  ) : (
                    <span class="min-w-0 flex-1 truncate">{view.annotation.label}</span>
                  )}
                  <button
                    type="button"
                    class="btn btn-ghost btn-xs"
                    aria-label="Copy QR content"
                    title="Copy QR content"
                    onClick={() => void copyText(view.annotation?.content ?? "")}
                  >
                    Copy
                  </button>
                </div>
              )}
            </li>
          );
        })}
      </ol>
    </aside>
  );
}
