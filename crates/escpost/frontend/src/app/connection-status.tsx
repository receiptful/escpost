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
      {scan.phase === "running" && <ScanProgress compact={compact} />}
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

function ScanProgress({ compact }: { compact: boolean }) {
  const { scan } = useAppData();
  // The probe total only arrives with the `prepared` event, which the server
  // sends once it has resolved the scan targets. Until then the bar is
  // indeterminate — daisyUI renders that from a missing `value` — because a
  // bar at 0 of 0 claims a precision the scan does not have yet.
  const preparing = scan.total === 0;
  // Not a live region, unlike the status block beside it: a sweep reports
  // hundreds of progress ticks, and announcing every one of them would bury
  // everything else. The `progress` element still exposes its value to
  // assistive technology on demand.
  return (
    <section
      aria-label="Printer discovery"
      class={compact ? "rounded-box bg-primary/10 px-3 py-2 text-xs" : "rounded-box bg-primary/10 p-4 text-sm"}
    >
      <p class="font-medium">Scanning printers</p>
      <progress
        aria-label="Scan progress"
        class="progress progress-primary mt-2 w-full"
        max={preparing ? undefined : scan.total}
        value={preparing ? undefined : scan.completed}
      />
      <p class="mt-1 flex items-center justify-between gap-2 text-base-content/70">
        <span>{preparing ? "Preparing…" : `${scan.completed} / ${scan.total}`}</span>
        <a class="link" href="/app/printers">View</a>
      </p>
    </section>
  );
}
