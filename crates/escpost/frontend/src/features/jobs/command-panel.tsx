import { copyText, webUrl } from "./annotation";
import { STICKY_HEADER } from "./reveal";
import { commandGroupView, type CommandGroup, type CommandGroupView } from "./model";

type Props = {
  groups: CommandGroup[];
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
          class={`flex w-[2.5ch] flex-col items-center leading-tight ${
            pairing && active === index ? "rounded-sm font-bold ring-1 ring-base-content/40" : ""
          }`}
          onPointerEnter={pairing ? () => onPreview(index) : undefined}
          onPointerLeave={pairing ? onPreviewEnd : undefined}
        >
          {pairing && (
            <span data-character={index} class="text-base-content">{byte.character}</span>
          )}
          <span data-hex={index}>{byte.hex}</span>
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
      aria-label="Commands in the current print job"
      class="max-h-[70vh] overflow-auto rounded-box border border-base-300 bg-base-100 xl:sticky xl:top-6 xl:max-h-[calc(100vh-8rem)]"
    >
      {/* The rows pass under the header, thus it needs a surface and a shadow
          of its own to stay apart from the row that scrolls behind it. */}
      <div
        {...{ [STICKY_HEADER]: "" }}
        class="sticky top-0 z-10 border-b border-base-content/10 bg-base-300 p-4 shadow-sm"
      >
        <h2 class="text-lg font-bold">Commands</h2>
        <p class="text-sm text-base-content/65">Hover or focus to preview. Click to pin.</p>
      </div>
      <ol class="divide-y divide-base-300">
        {props.groups.map((group, row) => {
          const view = commandGroupView(group);
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
                aria-label={`${view.name} ${view.byteStart}..${view.byteLast}: ${view.detail}`}
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
                  <span class="font-mono text-xs text-base-content/55">{view.byteStart}..{view.byteLast}</span>
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
                      class="flex min-w-0 flex-wrap items-center gap-x-1 gap-y-1 rounded border border-base-content/20 px-1.5 py-0.5 text-base-content/70"
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
