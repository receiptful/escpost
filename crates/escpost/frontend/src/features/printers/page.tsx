import { useCallback, useEffect, useRef, useState } from "preact/hooks";
import type { DiscoveryQuery } from "../../api/discovery-stream";
import type { AddPrinterBody, DiscoveredPrinter } from "../../api/types";
import { useAppData } from "../../app/data";
import { AddPrinterDialog } from "./add-printer-dialog";
import { DiscoveryPanel } from "./discovery-panel";
import { PrinterList } from "./printer-list";
import { ScanOptions } from "./scan-options";

// What `Discover printers` scans with before anyone opens the options panel:
// the CLI's own no-flag behaviour, both transports, targets detected
// automatically. It names no port and no timeout because nobody has chosen
// either, and the endpoint owns both defaults — a number restated here would
// be invisible in the interface and would silently outlive the server's own.
const DEFAULT_QUERY: DiscoveryQuery = { usb: true, network: true, subnets: [] };

export function PrintersPage() {
  const { scan, startScan, cancelScan, refreshPrinters, flashPrinter, markScanResultConfigured } = useAppData();
  const actions = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLUListElement>(null);
  const toggle = useRef<HTMLButtonElement>(null);
  const [query, setQuery] = useState<DiscoveryQuery>(DEFAULT_QUERY);
  const [menuOpen, setMenuOpen] = useState(false);
  const [optionsOpen, setOptionsOpen] = useState(false);
  // `null` while nothing is being registered, and `{ printer: null }` for the
  // manual dialog — `AddPrinterDialog` closes the native element in its
  // unmount cleanup, so dismissing it has to unmount it rather than blank a
  // field it is still reading.
  const [registering, setRegistering] = useState<{ printer: DiscoveredPrinter | null } | null>(null);

  // Dismissing the menu returns focus to the control that opened it, which is
  // the other half of the contract `role="menu"` announces: a reader who
  // opened it from the keyboard is not left with focus on a removed element.
  const closeMenu = useCallback((restoreFocus: boolean) => {
    setMenuOpen(false);
    if (restoreFocus) {
      toggle.current?.focus();
    }
  }, []);

  useEffect(() => {
    if (!menuOpen) {
      return;
    }
    const items = () => Array.from(menu.current?.querySelectorAll<HTMLButtonElement>("[role=\"menuitem\"]") ?? []);
    // Focus moves into the menu on open, so the next arrow key has somewhere
    // to move from and Escape has something to return.
    items()[0]?.focus();

    const dismiss = (event: Event) => {
      if (!actions.current?.contains(event.target as Node)) {
        closeMenu(false);
      }
    };
    const navigate = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        closeMenu(true);
        return;
      }
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp") {
        return;
      }
      event.preventDefault();
      const open = items();
      const current = open.indexOf(document.activeElement as HTMLButtonElement);
      const step = event.key === "ArrowDown" ? 1 : -1;
      // Wrapping in both directions, so the list has no dead end.
      open[(current + step + open.length) % open.length]?.focus();
    };
    document.addEventListener("pointerdown", dismiss);
    document.addEventListener("keydown", navigate);
    return () => {
      document.removeEventListener("pointerdown", dismiss);
      document.removeEventListener("keydown", navigate);
    };
  }, [menuOpen, closeMenu]);

  // A scan is started from two places with the same settings: the split
  // button reuses the last ones, the options panel supplies new ones and they
  // become the last ones.
  const beginScan = (next: DiscoveryQuery) => {
    setQuery(next);
    setMenuOpen(false);
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
            ref={toggle}
            type="button"
            class="btn btn-primary join-item"
            aria-label="Discovery options"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => {
              // The menu and the options panel are anchored to the same
              // corner, so one has to give way rather than render behind the
              // other, open and invisible.
              setOptionsOpen(false);
              setMenuOpen((open) => !open);
            }}
          >
            ▾
          </button>
        </div>

        {menuOpen && (
          // Each item's subtitle sits inside its button so the whole row stays
          // one target, and is hidden from the accessible name — which stays
          // the command — while `aria-describedby` still reads it out, since a
          // reference is followed into hidden content.
          <ul ref={menu} role="menu" class="absolute right-0 top-full z-20 mt-2 w-72 rounded-box bg-base-100 p-1 shadow-lg">
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
        scan={scan}
        onAdd={(printer) => setRegistering({ printer })}
        onCancel={() => {
          // Cancel discards the results along with the sweep: `cancelScan`
          // resets the scan to idle, so the panel unmounts and every printer
          // found so far goes with it. That is what Ctrl-C does to `printers
          // discover`, which also prints nothing for the run it interrupted,
          // and the alternative — stopping but keeping a partial list — is a
          // state the CLI has no equivalent of.
          cancelScan();
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
