import { useState } from "preact/hooks";
import type { DiscoveryQuery } from "../../api/discovery-stream";
import type { AddPrinterBody, DiscoveredPrinter } from "../../api/types";
import { useAppData } from "../../app/data";
import { AddPrinterDialog } from "./add-printer-dialog";
import { DiscoveryCard } from "./discovery-card";
import { DiscoveryPanel } from "./discovery-panel";
import { PrinterList } from "./printer-list";

export function PrintersPage() {
  // `scanQuery` is the scope the last scan ran with, and it comes from the
  // provider because this component does not survive a route change while the
  // scan does — `Rescan` after a detour must repeat the sweep that was
  // configured, not the default one.
  const { scan, scanQuery, startScan, cancelScan, markScanResultConfigured } = useAppData();
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
  };

  return (
    <section aria-labelledby="printers-heading" class="space-y-6">
      <h1 id="printers-heading" class="sr-only">Printers</h1>

      {/* One section for the whole of discovery, and one card inside it
          reading options, then results, then controls — what a scan would
          do, what the last one did, and what to do next. The results render
          nothing at all while the scan is idle, and the bar moves up to meet
          the accordion rather than leaving a hole. */}
      <section aria-labelledby="discovery-heading" class="space-y-2">
        <h2 id="discovery-heading" class="font-medium">Printer Discovery</h2>

        {/* The bar along the bottom belongs to the card; these two
            buttons are the page's, because both act on things only the page
            has: the scan and the registration dialog.

            The scope arrives as an argument rather than through state of our
            own, so the button below is built by the same render that draws
            the line stating that scope. There is no version of this page
            where the two disagree, not even for a frame. */}
        <DiscoveryCard
          query={scanQuery}
          open={optionsOpen}
          onOpenChange={setOptionsOpen}
          results={
            /* Whether USB was swept is the scope's fact, not the stream's:
               the stream reports progress, and only the query says which
               halves ran. `scanQuery` is the scope of the scan on screen,
               since it changes only when one starts. */
            <DiscoveryPanel scan={scan} usb={scanQuery.usb} onAdd={(printer) => setRegistering({ printer })} />
          }
          actions={(scope) => <>
            {/* The escape hatch for a printer no scan can reach. `IP` rather
                than `network` is the reader's word for it here and in the
                dialog it opens; the transport is still `network` on the wire
                and in the inventory's own column. */}
            <button
              type="button"
              class="btn btn-sm"
              onClick={() => setRegistering({ printer: null })}
            >
              Add IP printer manually
            </button>
            {/* One slot, three jobs. Start, stop and repeat are the same
                decision about the same scan, and the scope it acts on is the
                one the line above states — refused, like the scan itself,
                when that line states none. */}
            {running ? (
              <button
                type="button"
                class="btn btn-primary btn-sm"
                // Stops the probing and keeps what it found: the rows stay
                // listed and stay addable, and the line says where the sweep
                // was interrupted rather than claiming it finished.
                //
                // `printers discover` prints nothing for a run you Ctrl-C,
                // and this does not diverge from it: the operation both
                // interfaces drive is the same, and the difference is what
                // each can do with results it already holds. A terminated
                // process has nowhere left to put them; a page still on
                // screen does. Same shape as the reason/remedy rule — the
                // shared layer owns the fact, each interface owns what it
                // makes of it.
                onClick={cancelScan}
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
          </>}
        />
      </section>

      {/* The other named block. The heading belongs to the page rather than
          to the list, so it stands over the loading and error states too —
          an inventory that cannot be read is still the inventory. */}
      <section aria-labelledby="configured-printers-heading" class="space-y-2">
        <h2 id="configured-printers-heading" class="font-medium">Configured Printers</h2>
        <PrinterList />
      </section>

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
