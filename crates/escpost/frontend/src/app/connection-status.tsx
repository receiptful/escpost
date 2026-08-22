import type { ScanState } from "./data";
import { useAppData } from "./data";

// The shell's only global status surface: server reachability, plus the
// progress of whatever scan the application is running. A scan outlives the
// page that started it, so this is where its progress stays visible.
export function ConnectionStatus({ compact = false }: { compact?: boolean }) {
  const { connection, statusError, scan } = useAppData();
  const label = connection === "ready" ? "Ready" : connection === "disconnected" ? "Disconnected" : "Checking…";
  // The sidebar variant carries `mt-auto` on the pair rather than on the
  // status block, so adding progress above it keeps both anchored to the
  // bottom of the flex column instead of pushing the status block up.
  return (
    <div class={compact ? "space-y-2" : "mt-auto space-y-3"}>
      {/* Mounted at all times, unlike the progress block itself: a live
          region inserted into the document together with its text is not
          reliably announced, so the announcer has to outlive the scan it
          reports. It carries the start and the end of the scan and nothing
          in between, keeping the ticking readout out of any live region. */}
      <p aria-live="polite" class="sr-only">{scan.phase === "running" ? "Scanning printers" : ""}</p>
      {scan.phase === "running" && <ScanProgress compact={compact} scan={scan} />}
      <section
        aria-label="Server status"
        aria-live="polite"
        aria-atomic="true"
        class={compact ? "rounded-box bg-base-200 px-3 py-2 text-xs" : "rounded-box bg-base-200 p-4 text-sm"}
        role="status"
      >
        <p class="font-medium">Server status</p>
        <p class="mt-1 text-base-content/70">{label}</p>
        {statusError && <p role="alert" class="mt-2 text-warning">Status check unavailable: {statusError.message}</p>}
      </section>
    </div>
  );
}

// The compact header is a thin bar, so the same facts are laid out as an
// inline pill there rather than as a second stacked card that would roughly
// double its height. Both variants carry the label, the bar, the readout and
// the link back to the scan.
function ScanProgress({ compact, scan }: { compact: boolean; scan: ScanState }) {
  // A zero probe total means either that the `prepared` event has not landed
  // yet or that the scope resolved to no network targets at all — a USB-only
  // scan, which never sends a `progress` event. One neutral readout covers
  // both, where "Preparing…" would be a lie for the whole life of a USB-only
  // scan and "0 / 0" would be meaningless in either.
  const counted = scan.total > 0;
  const readout = counted ? `${scan.completed} / ${scan.total}` : "In progress…";
  const bar = (
    <progress
      aria-label="Scan progress"
      // daisyUI renders a missing `value` as the indeterminate animation,
      // which is what an unknown or unmeasurable total deserves.
      class={compact ? "progress progress-primary h-1.5 w-17" : "progress progress-primary mt-2 w-full"}
      max={counted ? scan.total : undefined}
      value={counted ? scan.completed : undefined}
    />
  );

  if (compact) {
    return (
      <section
        aria-label="Printer discovery"
        class="inline-flex max-w-full flex-wrap items-center gap-2 rounded-full bg-primary/10 px-3 py-1 text-xs font-medium text-primary"
      >
        <span>Scanning printers</span>
        {bar}
        <span>{readout}</span>
        <a class="link" href="/app/printers">View</a>
      </section>
    );
  }

  return (
    <section aria-label="Printer discovery" class="rounded-box bg-primary/10 p-4 text-sm text-primary">
      <p class="font-medium">Scanning printers</p>
      {bar}
      <p class="mt-1 flex items-center justify-between gap-2">
        <span>{readout}</span>
        <a class="link" href="/app/printers">View</a>
      </p>
    </section>
  );
}
