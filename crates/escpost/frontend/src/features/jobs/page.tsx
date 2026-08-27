import { useCallback, useEffect, useMemo, useRef, useState } from "preact/hooks";
import { CommandPanel } from "./command-panel";
import { groupJobCommands } from "./model";
import { revealWithin } from "./reveal";
import { SheetPreview } from "./sheet-preview";
import { useCurrentJob } from "./use-current-job";

const PAPER_MARGIN_KEY = "escpost.paper_margin";

function readPaperMargin() {
  try {
    const stored = localStorage.getItem(PAPER_MARGIN_KEY);
    return stored === null ? true : stored === "true";
  } catch {
    return true;
  }
}

export function JobsPage() {
  const resource = useCurrentJob();
  const job = resource.data?.job ?? null;
  const grouped = useMemo(() => job ? groupJobCommands(job) : null, [job]);
  const data = resource.data;
  const [previewedGroupId, setPreviewedGroupId] = useState<string | null>(null);
  const [pinnedGroupId, setPinnedGroupId] = useState<string | null>(null);
  // The character of the previewed group the pointer rests on, by its place in
  // the group. A byte and a printed character share that place.
  const [previewedCharacter, setPreviewedCharacter] = useState<number | null>(null);
  const [paperMargin, setPaperMargin] = useState(readPaperMargin);
  const [marginFlash, setMarginFlash] = useState(false);
  const sheetWorkspace = useRef<HTMLElement | null>(null);
  const commandPanel = useRef<HTMLElement | null>(null);
  const annotations = useRef(new Map<string, SVGElement>());
  const commands = useRef(new Map<string, HTMLElement>());
  const selectionJobId = useRef<string | null>(null);

  useEffect(() => {
    const nextJobId = job?.id ?? null;
    if (selectionJobId.current !== null && selectionJobId.current !== nextJobId) {
      setPreviewedGroupId(null);
      setPinnedGroupId(null);
      setPreviewedCharacter(null);
    }
    selectionJobId.current = nextJobId;
    annotations.current.clear();
    commands.current.clear();
  }, [job?.id]);

  useEffect(() => {
    const clearPin = (event: KeyboardEvent) => {
      if (event.key === "Escape") setPinnedGroupId(null);
    };
    window.addEventListener("keydown", clearPin);
    return () => window.removeEventListener("keydown", clearPin);
  }, []);

  const registerAnnotation = useCallback((id: string, element: SVGElement | null) => {
    if (element) annotations.current.set(id, element);
    else annotations.current.delete(id);
  }, []);
  const registerCommand = useCallback((id: string, element: HTMLElement | null) => {
    if (element) commands.current.set(id, element);
    else commands.current.delete(id);
  }, []);
  const previewFromCommand = useCallback((id: string) => {
    setPreviewedGroupId(id);
    revealWithin(annotations.current.get(id), sheetWorkspace.current, true);
  }, []);
  const previewFromAnnotation = useCallback((id: string) => {
    setPreviewedGroupId(id);
    revealWithin(commands.current.get(id), commandPanel.current, false);
  }, []);
  const endPreview = useCallback((id: string) => {
    setPreviewedGroupId((current) => current === id ? null : current);
    setPreviewedCharacter(null);
  }, []);
  const previewCharacter = useCallback((index: number) => setPreviewedCharacter(index), []);
  const endCharacterPreview = useCallback(() => setPreviewedCharacter(null), []);
  const pinFromCommand = useCallback((id: string) => {
    setPinnedGroupId(id);
    revealWithin(annotations.current.get(id), sheetWorkspace.current, true);
  }, []);
  const pinFromAnnotation = useCallback((id: string) => {
    setPinnedGroupId(id);
    revealWithin(commands.current.get(id), commandPanel.current, false);
  }, []);

  const changePaperMargin = (enabled: boolean) => {
    setPaperMargin(enabled);
    try {
      localStorage.setItem(PAPER_MARGIN_KEY, String(enabled));
    } catch {
      // The preference remains active for this page even when storage is blocked.
    }
    if (enabled) {
      setMarginFlash(false);
      requestAnimationFrame(() => setMarginFlash(true));
      window.setTimeout(() => setMarginFlash(false), 650);
    }
  };

  return (
    <section aria-labelledby="jobs-heading" class="flex flex-col gap-5 xl:min-h-0 xl:flex-1">
      <h1 id="jobs-heading" class="sr-only">Print jobs</h1>

      {!(job && grouped) && (
        <JobStatus
          resource={resource}
          paperMargin={paperMargin}
          onPaperMarginChange={changePaperMargin}
        />
      )}

      {job && grouped && (
        <div class="grid min-w-0 gap-5 xl:min-h-0 xl:flex-1 xl:grid-cols-[minmax(0,1fr)_24rem]">
          {/* The status covers the sheets alone, thus the bytes beside them
              start at the top of the page. */}
          <div data-sheet-column class="flex min-w-0 flex-col gap-5 xl:min-h-0">
          <JobStatus
            resource={resource}
            paperMargin={paperMargin}
            onPaperMarginChange={changePaperMargin}
          />
          <div
            ref={(element) => { sheetWorkspace.current = element; }}
            role="region"
            aria-label="Rendered receipt sheets"
            // The sheets take what the column has left once the status has
            // its share, thus no number here stands for the height of another
            // element.
            class="min-w-0 overflow-auto rounded-box border border-base-300 bg-base-200 p-4 xl:min-h-0 xl:flex-1"
          >
            <div class="flex flex-wrap items-start justify-start gap-8">
              {grouped.sheets.map((sheet) => (
                <SheetPreview
                  key={sheet.number}
                  sheet={sheet}
                  sheetCount={grouped.sheets.filter((candidate) => candidate.image_url).length}
                  antialias={job.antialias}
                  paperMargin={paperMargin}
                  marginFlash={marginFlash}
                  previewedGroupId={previewedGroupId}
                  pinnedGroupId={pinnedGroupId}
                  previewedCharacter={previewedCharacter}
                  onPreviewCharacter={previewCharacter}
                  onPreviewCharacterEnd={endCharacterPreview}
                  register={registerAnnotation}
                  onPreview={previewFromAnnotation}
                  onPreviewEnd={endPreview}
                  onPin={pinFromAnnotation}
                  onClearPin={() => setPinnedGroupId(null)}
                />
              ))}
              {grouped.sheets.every((sheet) => !sheet.image_url) && (
                <p class="py-12 text-base-content/65">This job contains commands but produced no printable sheet.</p>
              )}
            </div>
          </div>
          </div>
          {grouped.groups.length > 0 && (
            <CommandPanel
              groups={grouped.groups}
              byteCount={grouped.byteCount}
              inputUrl={data?.receiving ? undefined : job.input_url}
              styleDefaults={job.style_defaults}
              previewedGroupId={previewedGroupId}
              pinnedGroupId={pinnedGroupId}
              previewedCharacter={previewedCharacter}
              onPreviewCharacter={previewCharacter}
              onPreviewCharacterEnd={endCharacterPreview}
              panelRef={(element) => { commandPanel.current = element; }}
              register={registerCommand}
              onPreview={previewFromCommand}
              onPreviewEnd={endPreview}
              onPin={pinFromCommand}
            />
          )}
        </div>
      )}
    </section>
  );
}

function JobStatus({ resource, paperMargin, onPaperMarginChange }: {
  resource: ReturnType<typeof useCurrentJob>;
  paperMargin: boolean;
  onPaperMarginChange: (enabled: boolean) => void;
}) {
  const data = resource.data;
  const job = data?.job;
  return (
    <div class="space-y-3" aria-live="polite">
      <div role="group" aria-label="Current job status" class="flex flex-wrap items-center gap-2 rounded-box border border-base-300 bg-base-100 p-4">
        <span class="font-semibold">Profile</span>
        <span class="badge badge-outline">{data?.profile || "Unknown"}</span>
        {data?.receiving ? (
          <span class="badge badge-info">Receiving a job…</span>
        ) : job?.completed_at_unix_ms ? (
          <span class="text-sm text-base-content/70">
            Completed {new Date(job.completed_at_unix_ms).toLocaleTimeString()}
          </span>
        ) : null}
        {job?.completion === "timeout" && <span class="badge badge-warning">idle-timeout</span>}
        <label class="label ml-auto cursor-pointer gap-3 rounded-lg px-2 py-1">
          <span class="label-text">Paper margin</span>
          <input
            type="checkbox"
            class="toggle toggle-primary toggle-sm"
            checked={paperMargin}
            onChange={(event) => onPaperMarginChange(event.currentTarget.checked)}
          />
        </label>
      </div>
      {resource.loading && !data && <div class="skeleton h-20 w-full" aria-label="Loading current job" />}
      {resource.error && (
        <div class="alert alert-warning" role="status">
          <span>Job data is unavailable. Retrying automatically; the last preview remains visible.</span>
        </div>
      )}
      {data?.error && <div class="alert alert-error" role="alert"><span>Render error: {data.error}</span></div>}
      {job?.warnings.map((warning) => (
        <div key={warning} class="alert alert-warning"><span>{warning}</span></div>
      ))}
      {!resource.loading && data && !job && !data.error && (
        <div class="min-h-56 rounded-box border border-dashed border-base-300 bg-base-100 p-8">
          <h2 class="text-xl font-bold">Waiting for first job</h2>
          {data.hint && <p class="mt-2 text-base-content/65">{data.hint}</p>}
        </div>
      )}
    </div>
  );
}
