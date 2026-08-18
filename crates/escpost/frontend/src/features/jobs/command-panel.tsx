import { copyText, webUrl } from "./annotation";
import { commandGroupView, groupEffectSummary, type CommandGroup } from "./model";

type Props = {
  groups: CommandGroup[];
  previewedGroupId: string | null;
  pinnedGroupId: string | null;
  panelRef: (element: HTMLElement | null) => void;
  register: (id: string, element: HTMLElement | null) => void;
  onPreview: (id: string) => void;
  onPreviewEnd: (id: string) => void;
  onPin: (id: string) => void;
};

export function CommandPanel(props: Props) {
  return (
    <aside
      ref={props.panelRef}
      aria-label="Commands in the current print job"
      class="max-h-[70vh] overflow-auto rounded-box border border-base-300 bg-base-100 xl:sticky xl:top-6 xl:max-h-[calc(100vh-8rem)]"
    >
      <div class="sticky top-0 z-10 border-b border-base-300 bg-base-100 p-4">
        <h2 class="text-lg font-bold">Commands</h2>
        <p class="text-sm text-base-content/65">Hover or focus to preview. Click to pin.</p>
      </div>
      <ol class="divide-y divide-base-300">
        {props.groups.map((group) => {
          const view = commandGroupView(group);
          const previewed = props.previewedGroupId === group.id;
          const pinned = props.pinnedGroupId === group.id;
          const href = view.annotation ? webUrl(view.annotation.content) : null;
          return (
            <li key={group.id} class="p-2">
              <button
                ref={(element) => props.register(group.id, element)}
                type="button"
                aria-label={`${view.name} ${view.byteStart}..${view.byteEnd}: ${view.detail}`}
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
                  <span class="font-mono font-bold">{view.name}</span>
                  <span class="font-mono text-xs text-base-content/55">{view.byteStart}..{view.byteEnd}</span>
                </span>
                <span class="mt-1 block break-words text-sm">{view.detail}</span>
                {view.paintLifecycle === "buffered" && (
                  <span class="badge badge-warning badge-sm mt-2">Not printed</span>
                )}
                <span class="mt-2 block text-xs text-base-content/60">{groupEffectSummary(group)}</span>
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
