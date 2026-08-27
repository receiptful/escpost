import { useAppData } from "./data";
import { useServerStatus } from "./server-status-data";

// The shell's only global status surface: server reachability, plus whatever
// the application is busy with. Both activities outlive the page that shows
// their detail — a scan keeps running after you navigate away, and a print
// job arrives whichever page you happen to be on — so this is where their
// progress stays visible.
export function ServerStatus({ compact = false }: { compact?: boolean }) {
  const { scan } = useAppData();
  const { phase, snapshot, error } = useServerStatus();
  const label = phase === "ready" ? "Ready" : phase === "disconnected" ? "Disconnected" : "Checking…";
  const scanning = scan.phase === "running";
  // The status payload carries one bit for the virtual printer, and the
  // server holds it from the first byte of a job until its render has been
  // stored — `end_capture` runs after `finalize`. Arriving and rendering are
  // therefore the same state on the wire, and the current-job endpoint
  // reports that same bit, so no source here can tell them apart. The wording
  // covers the whole span rather than naming a phase nothing can prove.
  //
  // Gated on the connection because the last status snapshot survives the
  // server going away: without this the pill would go on reporting a job
  // arriving at a server nobody can reach, directly above a status block
  // saying the opposite. A scan needs no such guard — it is driven by a
  // stream that fails on its own when the server goes.
  const receivingJob = phase === "ready" && snapshot.virtual_printer?.state === "receiving";
  const busy = scanning || receivingJob;
  const activities = [
    scanning ? "Scanning printers" : null,
    receivingJob ? "Incoming print job" : null,
  ].filter((activity) => activity !== null);
  // The two activities are independent and can run at once, so they stack.
  // Their order is fixed rather than most-recent-first: a pill that jumps
  // position when the other one comes or goes is harder to follow than one
  // that stays put, and it is the same order the announcer reads them in.
  const pills = (
    <>
      {scanning && (
        <ActivityProgress
          barLabel="Scan progress"
          compact={compact}
          completed={scan.total > 0 ? scan.completed : undefined}
          href="/printers"
          label="Scanning printers"
          region="Printer discovery"
          // A zero probe total means either that the `prepared` event has not
          // landed yet or that the scope resolved to no network targets at
          // all — a USB-only scan, which never sends a `progress` event. One
          // neutral readout covers both, where "Preparing…" would be a lie
          // for the whole life of a USB-only scan and "0 / 0" would be
          // meaningless in either.
          readout={scan.total > 0 ? `${scan.completed} / ${scan.total}` : "In progress…"}
          total={scan.total > 0 ? scan.total : undefined}
        />
      )}
      {receivingJob && (
        <ActivityProgress
          barLabel="Print job progress"
          compact={compact}
          href="/jobs"
          label="Incoming print job"
          region="Print job"
          // No readout: nothing measures a job's size, so the only thing this
          // row could say is that it is in progress, which the label and the
          // indeterminate bar already say. Two pills can be live at once, and
          // the compact header has no width to spend restating a fact.
        />
      )}
    </>
  );

  // The sidebar variant carries `mt-auto` on the whole set rather than on the
  // status block, so adding progress above it keeps everything anchored to
  // the bottom of the flex column instead of pushing the status block up.
  return (
    <div class={compact ? "space-y-2" : "mt-auto space-y-3"}>
      {/* Mounted at all times, unlike the pills themselves: a live region
          inserted into the document together with its text is not reliably
          announced, so the announcer has to outlive the activities it
          reports. One region for both, rather than one each, so a reader
          hears what the application is busy with as a single sentence
          instead of two regions competing. It carries the start and the end
          of each activity and nothing in between, keeping the ticking
          readout out of any live region. */}
      <p aria-live="polite" class="sr-only">{activities.join(". ")}</p>
      {compact
        // Inline pills, so they share a row wherever the header is wide
        // enough for both and only wrap onto a second line when it is not.
        // The sidebar's cards are full width and can only ever stack, so
        // they stay direct children of the anchored column.
        ? busy && <div class="flex flex-wrap items-center gap-2">{pills}</div>
        : pills}
      <section
        aria-label="Server status"
        aria-live="polite"
        aria-atomic="true"
        class={compact ? "rounded-box bg-base-200 px-3 py-2 text-xs" : "rounded-box bg-base-200 p-4 text-sm"}
        role="status"
      >
        <p class="font-medium">Server status</p>
        <p class="mt-1 text-base-content/70">{label}</p>
        {phase === "disconnected" && <p role="alert" class="mt-2 text-warning">Status check unavailable: {error.message}</p>}
      </section>
    </div>
  );
}

type ActivityProgressProps = {
  // Labels the bar itself, so a reader that lands on it out of context knows
  // whose progress it is.
  barLabel: string;
  compact: boolean;
  // Omitted together with `total` for an activity whose size is unknown.
  completed?: number;
  href: string;
  label: string;
  // Names the region, which is what a reader navigating by landmark hears.
  region: string;
  readout?: string;
  total?: number;
};

// One shape for every activity the shell reports, because both are the same
// thing: something long-running is happening elsewhere, here is roughly how
// far along it is, and here is the way back to the page that owns it. Sharing
// the component rather than copying it keeps the two from drifting apart the
// next time either layout is touched.
//
// The compact header is a thin bar, so the same facts are laid out as an
// inline pill there rather than as a second stacked card that would roughly
// double its height.
function ActivityProgress({ barLabel, compact, completed, href, label, region, readout, total }: ActivityProgressProps) {
  const bar = (
    <progress
      aria-label={barLabel}
      // daisyUI renders a missing `value` as the indeterminate animation,
      // which is what an unknown or unmeasurable total deserves.
      class={compact ? "progress progress-primary h-1.5 w-17" : "progress progress-primary mt-2 w-full"}
      max={total}
      value={completed}
    />
  );
  const view = <a class="link" href={href}>View</a>;

  if (compact) {
    return (
      <section
        aria-label={region}
        class="inline-flex max-w-full flex-wrap items-center gap-2 rounded-full bg-primary/10 px-3 py-1 text-xs font-medium text-primary"
      >
        <span>{label}</span>
        {bar}
        {readout && <span>{readout}</span>}
        {view}
      </section>
    );
  }

  return (
    <section aria-label={region} class="rounded-box bg-primary/10 p-4 text-sm text-primary">
      <p class="font-medium">{label}</p>
      {bar}
      {/* The readout keeps its span even when there is nothing to report, so
          `justify-between` still holds the link against the right edge. */}
      <p class="mt-1 flex items-center justify-between gap-2">
        <span>{readout}</span>
        {view}
      </p>
    </section>
  );
}
