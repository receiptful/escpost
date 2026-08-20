import { useEffect, useRef, useState } from "preact/hooks";
import type { DiscoveryQuery } from "../../api/discovery-stream";
import type { DiscoveredPrinter } from "../../api/types";
import { useAppData } from "../../app/data";
import { AddPrinterDialog } from "./add-printer-dialog";
import { DiscoveryPanel } from "./discovery-panel";
import { PrinterList } from "./printer-list";
import { ScanOptions } from "./scan-options";

// What `Discover printers` scans with before anyone opens the options panel:
// the CLI's own no-flag behaviour — both transports, targets detected
// automatically — at the shared layer's defaults, copied from
// `printers::discover::http`'s `DEFAULT_PORT` and `DEFAULT_TIMEOUT_MS`. The
// options panel reads the server's own values and replaces these as soon as
// it starts a scan, so drift from the Rust constants costs at most one scan
// at a stale port, never a divergence that outlives it.
const DEFAULT_QUERY: DiscoveryQuery = { usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1_000 };

// A printer registered from the results panel, paired with the name it was
// registered under. Held by object identity, which is what makes it right:
// the provider appends the object the stream delivered and never rebuilds it,
// and a rescan replaces the whole array, so entries from a previous scan stop
// matching on their own.
type Registration = { printer: DiscoveredPrinter; name: string };

export function PrintersPage() {
  const { scan, startScan, cancelScan, refreshPrinters, flashPrinter } = useAppData();
  const actions = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState<DiscoveryQuery>(DEFAULT_QUERY);
  const [menuOpen, setMenuOpen] = useState(false);
  const [optionsOpen, setOptionsOpen] = useState(false);
  // `null` while nothing is being registered, and `{ printer: null }` for the
  // manual dialog — `AddPrinterDialog` closes the native element in its
  // unmount cleanup, so dismissing it has to unmount it rather than blank a
  // field it is still reading.
  const [registering, setRegistering] = useState<{ printer: DiscoveredPrinter | null } | null>(null);
  const [registered, setRegistered] = useState<Registration[]>([]);

  useEffect(() => {
    if (!menuOpen) {
      return;
    }
    const dismiss = (event: Event) => {
      if (!actions.current?.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setMenuOpen(false);
      }
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", escape);
    };
  }, [menuOpen]);

  // A scan is started from two places with the same settings: the split
  // button reuses the last ones, the options panel supplies new ones and they
  // become the last ones.
  const beginScan = (next: DiscoveryQuery) => {
    setQuery(next);
    setRegistered([]);
    setMenuOpen(false);
    setOptionsOpen(false);
    startScan(next);
  };

  const handleAdded = (name: string) => {
    const added = registering?.printer ?? null;
    setRegistering(null);
    if (added) {
      setRegistered((current) => [...current, { printer: added, name }]);
    }
    // A printer that was just registered has no availability transition to
    // diff against — it is absent from the previous inventory and present in
    // the next — so the flash is raised here and the row it lands on carries
    // it from the moment the refresh renders it.
    flashPrinter(name, "found");
    void refreshPrinters();
  };

  // A registered result leaves the panel by becoming what it now is: a
  // printer this machine has configured. The panel already hides those and
  // counts them, so the row disappears and the count line records the move
  // without the panel needing a second notion of "added".
  const panelScan = registered.length === 0 ? scan : {
    ...scan,
    printers: scan.printers.map((printer) => {
      const entry = registered.find((candidate) => candidate.printer === printer);
      return entry ? { ...printer, configured_names: [...printer.configured_names, entry.name] } : printer;
    }),
  };

  return (
    <section aria-labelledby="printers-heading" class="space-y-6">
      <h1 id="printers-heading" class="sr-only">Printers</h1>

      <div ref={actions} class="relative flex justify-end">
        {/* One action, with everything that changes it attached to it. Full
            width on a phone, where it is the only thing in the header. */}
        <div class="join w-full sm:w-auto">
          <button
            type="button"
            class="btn btn-primary join-item grow sm:grow-0"
            onClick={() => beginScan(query)}
          >
            {scan.phase === "idle" ? "Discover printers" : "Rescan"}
          </button>
          <button
            type="button"
            class="btn btn-primary join-item"
            aria-label="Discovery options"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          >
            ▾
          </button>
        </div>

        {menuOpen && (
          // Each item's subtitle sits inside its button so the whole row stays
          // one target, and is hidden from the accessible name — which stays
          // the command — while `aria-describedby` still reads it out, since a
          // reference is followed into hidden content.
          <ul role="menu" class="absolute right-0 top-full z-20 mt-2 w-72 rounded-box bg-base-100 p-1 shadow-lg">
            <li role="none">
              <button
                type="button"
                role="menuitem"
                class="w-full rounded-box px-3 py-2 text-left text-sm hover:bg-base-200"
                aria-describedby="discovery-menu-options-hint"
                onClick={() => {
                  setMenuOpen(false);
                  setOptionsOpen(true);
                }}
              >
                Scan options…
                <span id="discovery-menu-options-hint" aria-hidden="true" class="block text-xs text-base-content/60">
                  Transports, networks, port, timeout
                </span>
              </button>
            </li>
            <li role="separator" class="my-1 border-t border-base-300" />
            <li role="none">
              <button
                type="button"
                role="menuitem"
                class="w-full rounded-box px-3 py-2 text-left text-sm hover:bg-base-200"
                aria-describedby="discovery-menu-manual-hint"
                onClick={() => {
                  setMenuOpen(false);
                  setRegistering({ printer: null });
                }}
              >
                Add network printer manually
                <span id="discovery-menu-manual-hint" aria-hidden="true" class="block text-xs text-base-content/60">
                  For a printer the scan cannot reach
                </span>
              </button>
            </li>
          </ul>
        )}

        {optionsOpen && (
          <div class="absolute right-0 top-full z-20 mt-2 flex w-full max-w-sm justify-end">
            <ScanOptions onStart={beginScan} onClose={() => setOptionsOpen(false)} />
          </div>
        )}
      </div>

      {/* Deliberately unwrapped: the panel renders nothing at all while the
          scan is idle, and a heading or spacing box around it would leave an
          empty block above the inventory on first load. */}
      <DiscoveryPanel
        scan={panelScan}
        onAdd={(printer) => setRegistering({ printer })}
        onCancel={() => {
          // Cancel discards the results along with the sweep: `cancelScan`
          // resets the scan to idle, so the panel unmounts and every printer
          // found so far goes with it. That is what Ctrl-C does to `printers
          // discover`, which also prints nothing for the run it interrupted,
          // and the alternative — stopping but keeping a partial list — is a
          // state the CLI has no equivalent of.
          cancelScan();
          setRegistered([]);
        }}
      />

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
