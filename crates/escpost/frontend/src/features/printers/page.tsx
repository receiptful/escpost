import { useState } from "preact/hooks";
import type { DiscoveryQuery } from "../../api/discovery-stream";
import type { AddPrinterBody, DiscoveredPrinter } from "../../api/types";
import { useAppData } from "../../app/data";
import { AddPrinterDialog } from "./add-printer-dialog";
import { DiscoveryPanel } from "./discovery-panel";
import { PrinterList } from "./printer-list";
import { ScanOptions } from "./scan-options";

export function PrintersPage() {
  // `scanQuery` is the scope the last scan ran with, and it comes from the
  // provider because this component does not survive a route change while the
  // scan does — `Rescan` after a detour must repeat the sweep that was
  // configured, not the default one.
  const { scan, scanQuery, startScan, cancelScan, refreshPrinters, flashPrinter, markScanResultConfigured } = useAppData();
  // The scope the scan options currently state, or `null` while they state
  // none. It is where the one scan button gets its query, so the button and
  // the line above the form cannot mean two different sweeps.
  const [scope, setScope] = useState<DiscoveryQuery | null>(null);
  const [optionsOpen, setOptionsOpen] = useState(false);
  // `null` while nothing is being registered, and `{ printer: null }` for the
  // manual dialog — `AddPrinterDialog` closes the native element in its
  // unmount cleanup, so dismissing it has to unmount it rather than blank a
  // field it is still reading.
  const [registering, setRegistering] = useState<{ printer: DiscoveredPrinter | null } | null>(null);

  const running = scan.phase === "running";

  // Starting shuts the form: it has done its job, and the progress and
  // results it produces need the room. This is the only place a scan starts,
  // so it is the only place that has to remember.
  const beginScan = (next: DiscoveryQuery) => {
    setOptionsOpen(false);
    startScan(next);
  };

  const handleAdded = (name: string, connection: AddPrinterBody["connection"]) => {
    setRegistering(null);
    // A registered result leaves the panel by becoming what it now is: a
    // printer this machine has configured. The scan owns that fact, so a
    // route change cannot undo it.
    markScanResultConfigured(name, connection);
    // Forced, because the inventory poll that may be in flight was issued
    // before this printer existed and cannot report it.
    //
    // The flash is raised once that has landed rather than now: a forced
    // refresh waits for the in-flight poll and then makes a request of its
    // own, and a poll can take seconds when the backend is confirming a
    // printer unreachable. Raised now, the window could expire before the row
    // it belongs to existed. A printer that was just registered has no
    // availability transition to diff against — it is absent from the
    // previous inventory and present in the next — so this is the only place
    // the flash can come from.
    void refreshPrinters({ force: true }).then(() => flashPrinter(name, "found"));
  };

  return (
    <section aria-labelledby="printers-heading" class="space-y-6">
      <h1 id="printers-heading" class="sr-only">Printers</h1>

      {/* One section for the whole of discovery: what a scan would do, the
          button that does it, and what it found. The results panel renders
          nothing at all while the scan is idle, and the header and options
          above it stand on their own. */}
      <section aria-labelledby="discovery-heading" class="space-y-2">
        {/* Wraps rather than crushes: the manual-add label is long, and at
            phone width the buttons take a row of their own, still trailing. */}
        <header class="flex flex-wrap items-center gap-2">
          <h2 id="discovery-heading" class="font-medium">Discovery</h2>
          <div class="ml-auto flex gap-2">
            {/* The escape hatch for a printer no scan can reach, so it sits
                beside the scan rather than inside anything. */}
            <button
              type="button"
              class="btn btn-sm"
              onClick={() => setRegistering({ printer: null })}
            >
              Add network printer manually
            </button>
            {/* One slot, three jobs. Start, stop and repeat are the same
                decision about the same scan, and the scope it acts on is the
                one the line below states — refused, like the scan itself,
                when that line states none. */}
            {running ? (
              <button
                type="button"
                class="btn btn-primary btn-sm"
                onClick={() => {
                  // Cancel discards the results along with the sweep:
                  // `cancelScan` resets the scan to idle, so the panel
                  // unmounts and every printer found so far goes with it.
                  // That is what Ctrl-C does to `printers discover`, which
                  // also prints nothing for the run it interrupted, and the
                  // alternative — stopping but keeping a partial list — is a
                  // state the CLI has no equivalent of.
                  cancelScan();
                }}
              >
                Cancel
              </button>
            ) : (
              <button
                type="button"
                class="btn btn-primary btn-sm"
                disabled={scope === null}
                onClick={() => scope && beginScan(scope)}
              >
                {scan.phase === "idle" ? "Scan" : "Rescan"}
              </button>
            )}
          </div>
        </header>

        <ScanOptions
          query={scanQuery}
          open={optionsOpen}
          onOpenChange={setOptionsOpen}
          onScopeChange={setScope}
        />

        <DiscoveryPanel scan={scan} onAdd={(printer) => setRegistering({ printer })} />
      </section>

      <PrinterList />

      {registering && (
        <AddPrinterDialog
          printer={registering.printer}
          onClose={() => setRegistering(null)}
          onAdded={handleAdded}
        />
      )}
    </section>
  );
}
