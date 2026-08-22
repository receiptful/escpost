import { useEffect, useRef, useState } from "preact/hooks";
import type { UsbDiscoveryFailure } from "../../api/discovery-stream";
import type { DiscoveredPrinter, UsbConnection } from "../../api/types";
import type { ScanState } from "../../app/data";
import { countOf } from "./counts";
import { endpointHex, usbHex } from "./usb";

// How long an arriving row keeps the flash class. The one thing this number
// owes anything to is the animation in `styles.css`, which it has to outlast:
// removing the class mid-animation would cut the fade off. Nothing couples it
// to the window the inventory's own flashes use.
const FLASH_DURATION = 1_200;

// The facts the add dialog will show read-only, stated the way `printers
// discover` states them, so the same device reads the same in both
// interfaces. The endpoints are here because they are the one thing about a
// USB device the reader still has to choose between.
function usbFacts(connection: UsbConnection) {
  const parts = [`USB ${usbHex(connection.vendor_id)}:${usbHex(connection.product_id)}`];
  if (connection.bus && connection.address !== null) {
    parts.push(`bus ${connection.bus} addr ${String(connection.address).padStart(3, "0")}`);
  }
  parts.push(connection.serial_number ? `serial ${connection.serial_number}` : "no serial");
  parts.push(`interface ${connection.interface_number}`);
  if (connection.out_endpoints.length > 0) {
    parts.push(`out ${connection.out_endpoints.map(endpointHex).join(", ")}`);
  }
  return parts.join(" · ");
}

// Identity of a discovered printer across the events of one scan: the facts
// that make it the same device rather than the same row. A rescan clears the
// list, so this never has to survive one.
function printerKey(printer: DiscoveredPrinter) {
  const connection = printer.connection;
  return connection.type === "network"
    ? `network:${connection.host}:${connection.port}`
    : `usb:${connection.vendor_id}:${connection.product_id}:${connection.serial_number ?? ""}:${connection.bus ?? ""}:${connection.address ?? ""}:${connection.interface_number}`;
}

function printerTitle(printer: DiscoveredPrinter) {
  const connection = printer.connection;
  if (connection.type === "network") {
    return `${connection.host}:${connection.port}`;
  }
  return connection.product ?? connection.manufacturer ?? `USB ${usbHex(connection.vendor_id)}:${usbHex(connection.product_id)}`;
}

function printerFacts(printer: DiscoveredPrinter) {
  return printer.connection.type === "network"
    ? printer.interface ? `Network · reachable via ${printer.interface}` : "Network"
    : usbFacts(printer.connection);
}

// The same sentence `printers discover` writes to its warning stream, minus
// its `Warning:` prefix, which the banner already carries visually.
function failureSentence(failure: UsbDiscoveryFailure) {
  const action = failure.stage === "open_device"
    ? "Could not open"
    : "Could not inspect the active configuration of";
  return `${action} USB device ${usbHex(failure.vendor_id)}:${usbHex(failure.product_id)}: ${failure.reason}.`;
}

/**
 * The discovery results, from the first streamed printer to the completed
 * scan. It renders `ScanState` and nothing else, so it is as correct halfway
 * through a sweep — USB results already listed, network probes still
 * running — as it is at the end of one.
 *
 * Only printers that are not yet configured become rows. Already-configured
 * hits are counted here and reported by flashing the row they already occupy
 * in the inventory below, which the application data provider drives.
 *
 * It renders into the discovery card rather than owning one, between the
 * scan options above it and the controls below: the section around that
 * card is already titled `Printer Discovery`, and the button in that bar
 * starts, stops and repeats the scan
 * reported here. No card chrome of its own, or one block would read as two.
 *
 * `usb` is the one fact about the scan that the stream does not carry: it
 * reports progress, not the query that produced it, so whether the USB half
 * ran at all has to come from the scope.
 */
export function DiscoveryPanel({ scan, usb, onAdd }: {
  scan: ScanState;
  usb: boolean;
  onAdd: (printer: DiscoveredPrinter) => void;
}) {
  // Keys seen on the previous render, or `null` until the first one: a panel
  // that mounts onto a scan already in progress must not flash every row
  // that arrived while it was unmounted, since in-app navigation leaves the
  // scan running.
  const seen = useRef<Set<string> | null>(null);
  // Keyed by the batch that scheduled it, so a settled batch drops its handle
  // instead of the map growing for every arrival of a long scan.
  const timeouts = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map());
  const batches = useRef(0);
  const [flashing, setFlashing] = useState<string[]>([]);
  const discovered = scan.printers;

  useEffect(() => {
    const keys = discovered.map(printerKey);
    const previous = seen.current;
    seen.current = new Set(keys);
    // A rescan empties the list, and a printer found again by the next scan
    // is a new arrival again.
    if (previous === null || keys.length === 0) {
      return;
    }
    const arrivals = keys.filter((key) => !previous.has(key));
    if (arrivals.length === 0) {
      return;
    }
    setFlashing((current) => [...current, ...arrivals]);
    const batch = batches.current++;
    const timeout = setTimeout(() => {
      timeouts.current.delete(batch);
      setFlashing((current) => current.filter((key) => !arrivals.includes(key)));
    }, FLASH_DURATION);
    timeouts.current.set(batch, timeout);
  }, [discovered]);

  useEffect(() => {
    const pending = timeouts.current;
    return () => {
      for (const timeout of pending.values()) {
        clearTimeout(timeout);
      }
      pending.clear();
    };
  }, []);

  if (scan.phase === "idle") {
    return null;
  }

  const unconfigured = discovered.filter((printer) => printer.configured_names.length === 0);
  const configuredCount = discovered.length - unconfigured.length;
  // USB first whatever the arrival order: an enumerated device is a fact
  // about this machine, and it is the part of a scan that is already final
  // while the sweep underneath is still guessing.
  const rows = [
    ...unconfigured.filter((printer) => printer.transport === "usb"),
    ...unconfigured.filter((printer) => printer.transport !== "usb"),
  ];

  const running = scan.phase === "running";
  const grantable = scan.failures.some((failure) => failure.permission_denied && failure.can_grant_usb_permissions);
  // How many printers answered, which is every printer the scan reported and
  // not only the ones that became rows: a reader watching a sweep wants to
  // know the network replied at all, and a printer they registered last week
  // replying is still a printer replying. Rising from zero, so it says that
  // during the sweep rather than only at the end of one.
  //
  // How many of those are worth acting on, in the same breath and from the
  // same list, so the two numbers cannot come apart. The already-configured
  // count is the difference between them and is not stated a third time.
  //
  // The new count is carried even when it equals the total, because a rule
  // that hid it sometimes would make its absence mean two things — but not
  // at zero, where nothing answered and `(0 new)` is noise. It is stated
  // while the sweep is still running too, because that is when a reader
  // registers something: a printer added mid-scan leaves the results, and
  // this line is the only place that says where it went.
  const found = `${countOf(discovered.length, "printer")} found`;
  const summary = discovered.length === 0 ? found : `${found} (${unconfigured.length} new)`;
  // The other half of the line is the scan rather than the printers: which
  // halves of it ran, and how far the network one has got. Both transports
  // selected with nothing plugged in used to read exactly like a scan where
  // USB never ran, which is the one thing this states that nothing else can.
  //
  // `Checking` while running and `Checked` when done, because nothing on the
  // stream says enumeration has finished — USB results simply arrive before
  // the sweep does — so the past tense would be claiming knowledge the panel
  // does not have.
  //
  // Deliberately no count of USB devices, ports or controllers. `list()` asks
  // the OS to enumerate printer-class devices; there is no set of N things
  // probed, so a number there would exist only to look symmetrical with the
  // address count — which is there as a cost signal, and USB enumeration
  // costs nothing.
  //
  // The network half states probe counts, never networks or ports: the scan
  // state carries what the stream reported, and the stream reports progress
  // rather than the query that produced it.
  // A stopped scan keeps the shape of the line the reader was already
  // watching: same `N / M`, same place, only the verb settling from present
  // to past. Saying it in words as well — "stopped after 514 of 1,012" —
  // would change the verb, the preposition and the separator at once, and
  // make the eye re-parse a line it had been tracking. That the sweep was
  // cut short is already legible from the count falling short of the total,
  // the progress bar being gone and the button reading `Rescan`.
  const stopped = scan.phase === "stopped";
  const usbLine = usb ? (running ? "checking USB" : "checked USB") : "";
  const networkLine = scan.total === 0
    ? ""
    : running
      // `IP addresses` in both phases and both interfaces of this line: one
      // quantity said two ways — hosts here, addresses there — is drift a
      // reader has to translate. `IP` because this panel also counts USB
      // devices and printers, and the CLI does not need the word because it
      // has just printed the subnets it is about to sweep.
      ? `scanning ${scan.completed.toLocaleString()} / ${scan.total.toLocaleString()} IP addresses`
      : stopped
        ? `scanned ${scan.completed.toLocaleString()} / ${scan.total.toLocaleString()} IP addresses`
        : `scanned ${scan.total.toLocaleString()} IP addresses`;
  const halves = [usbLine, networkLine].filter((half) => half.length > 0).join(" · ");
  // A scan with neither half to report is one whose targets have not arrived
  // yet, or a network-only scan that resolved to none.
  const scanLine = halves === ""
    ? running ? "Scanning for printers…" : stopped ? "Scan stopped" : "Scan complete"
    : `${halves[0]!.toUpperCase()}${halves.slice(1)}`;

  return (
    <div class="border-t border-base-300">
      <div class="space-y-2 px-4 py-3 text-sm">
        {/* One line, two halves: the network on the left, the printers on the
            right. They used to be two displays saying overlapping things —
            a total beside a breakdown of the same total — and folding them
            leaves one number to keep true.

            The live region is that count rather than the rows: a printer
            arriving is announced once, as one more result, instead of a
            screen reader narrating every fact of every row that lands. */}
        <div class="flex flex-wrap items-center justify-between gap-x-3 gap-y-1">
          <span>{scanLine}</span>
          <span aria-live="polite" class="text-base-content/70">{summary}</span>
        </div>
        {/* Only while there is progress to show. A full bar under a finished
            scan reports nothing the line above has not, and competes with
            the results for the eye. */}
        {running && scan.total > 0 && (
          <progress class="progress progress-primary w-full" value={scan.completed} max={scan.total} aria-label="Scan progress" />
        )}
      </div>

      {scan.error && (
        <p role="alert" class="alert alert-error alert-soft rounded-none text-sm">{scan.error}</p>
      )}

      {scan.failures.length > 0 && (
        // A USB enumeration failure is tolerated rather than fatal, so
        // several can arrive and the scan carries on regardless.
        <div role="alert" class="alert alert-warning alert-soft block rounded-none text-sm">
          <ul class="space-y-1">
            {/* Two of the same model refused at two addresses are two
                failures reporting identical facts — `UsbEnumerationFailure`
                carries no bus or address — so only the position tells them
                apart. The list is append-only and never reordered. */}
            {scan.failures.map((failure, index) => (
              <li key={index}>{failureSentence(failure)}</li>
            ))}
          </ul>
          {/* The one place the browser names a terminal command: USB
              permissions cannot be fixed from a web page, so there is no
              in-app remedy to point at instead. One line for the whole
              banner, as the CLI also prints it once — and only where the
              command exists, which the server reports, since `printers
              grant-usb-permissions` is Linux-only and the browser may be
              talking to a machine across the room rather than this one. */}
          {grantable && (
            <p class="mt-2">
              Fix USB permissions on the machine running <code class="font-mono">escpost serve</code>, with:{" "}
              <code class="font-mono">sudo escpost printers grant-usb-permissions</code>
            </p>
          )}
        </div>
      )}

      {rows.map((printer) => {
        const key = printerKey(printer);
        const title = printerTitle(printer);
        return (
          // Three columns: the badge, the two lines about one printer, and
          // the button. The badge used to sit inside the first line, which
          // left the second line starting under it rather than under the
          // host it describes.
          //
          // The badge and the button both centre against the row rather than
          // sitting against its first line: they label and act on the whole
          // result, not on the host line, and one centred beside the other
          // aligned to the top would read as an accident. Centring is safe
          // here because the middle column is always exactly two lines — both
          // of its lines truncate rather than wrap — so the badge can never
          // drift away from the printer it labels, however narrow the row.
          //
          // No width is fixed for the badge: it is text, and a hard column
          // would either clip a longer word or leave a gap beside a shorter
          // one. The middle column takes what is left and truncates inside
          // itself, so a long interface name or hostname cannot push the
          // button off the row.
          <div
            key={key}
            class={`flex items-center gap-3 border-t border-base-300 px-4 py-3 ${flashing.includes(key) ? "printer-row-found" : ""}`}
          >
            <span class="badge badge-primary badge-sm shrink-0">New</span>
            <div class="min-w-0 grow">
              <h3 class="truncate font-medium">{title}</h3>
              <p class="truncate font-mono text-xs text-base-content/60">{printerFacts(printer)}</p>
            </div>
            <button type="button" class="btn btn-primary btn-sm shrink-0" aria-label={`Add ${title}`} onClick={() => onAdd(printer)}>Add</button>
          </div>
        );
      })}

      {/* Nothing out there and everything out there already registered are
          different answers, and only the second one sends the reader to the
          inventory. A failed scan claims neither. */}
      {rows.length === 0 && scan.phase === "done" && (
        <div class="border-t border-base-300 px-4 py-8 text-center text-sm text-base-content/70">
          <p class="font-medium text-base-content">{configuredCount > 0 ? "No new printers" : "No printers discovered"}</p>
          <p class="mx-auto mt-1 max-w-prose">
            {configuredCount === 1
              ? "The one printer discovered is already configured. It is listed below with live status."
              : configuredCount > 0
                ? `All ${configuredCount} discovered printers are already configured. They are listed below with live status.`
                : "Nothing answered this scan."}
          </p>
        </div>
      )}
    </div>
  );
}
