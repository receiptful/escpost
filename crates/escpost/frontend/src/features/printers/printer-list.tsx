import type { Printer, UsbConnection } from "../../api/types";
import type { PrinterFlashes } from "../../app/printer-inventory-data";
import { usePrinterInventory } from "../../app/printer-inventory-data";
import { usbHex } from "./usb";

const timestampFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: "medium",
  timeStyle: "medium",
});

// The highlight a printer is currently carrying, or no class at all. Both
// layouts render the same printer, so both get the same treatment: the class
// goes on the `<tr>` and on the mobile `<article>`, which are the elements
// that own the row's background in each.
function flashClass(flashes: PrinterFlashes, name: string) {
  const flash = flashes[name];
  return flash === undefined ? "" : `printer-row-${flash}`;
}

// `IP` rather than `Network`, following the words the rest of the interface
// uses for a printer reached over the network — `Add IP printer manually`,
// `Network (IP) Printers`. The API field is still `network`, and so is
// `--transport network`: this is the reader's name for the fact, not the
// wire's.
function transportTag(transport: string) {
  return transport === "usb" ? "USB" : "IP";
}

function titleCase(value: string) {
  return `${value[0]?.toUpperCase()}${value.slice(1)}`;
}

function usbConnection(connection: UsbConnection) {
  const location = connection.bus && connection.address !== null ? `, bus ${connection.bus} address ${connection.address}` : "";
  const serial = connection.serial_number ? `, serial ${connection.serial_number}` : "";
  return `USB ${usbHex(connection.vendor_id)}:${usbHex(connection.product_id)}${location}${serial}, interface ${connection.interface_number}`;
}

function connectionFacts(printer: Printer) {
  return printer.connection.type === "network"
    ? `${printer.connection.host}:${printer.connection.port}`
    : usbConnection(printer.connection);
}

// How a printer is reached, as one fact: the transport as a quiet tag, then
// the address it names. Ghost rather than primary — it labels something,
// where the results panel's `New` badge asks to be acted on and has to draw
// the eye first.
//
// The tag is two or three letters to the eye and a word to a screen reader,
// which would otherwise read "IP" as a stray token in front of a number.
// Inline rather than flex, so a long USB connection string wraps after the
// tag the way it always did.
function connection(printer: Printer) {
  return (
    <>
      <span class="badge badge-ghost badge-sm mr-2 align-middle">{transportTag(printer.transport)}</span>
      <span class="sr-only">connection</span>
      {connectionFacts(printer)}
    </>
  );
}

export function PrinterList() {
  const resource = usePrinterInventory();
  const snapshot = resource.snapshot;
  const printerData = snapshot?.printers;

  if (!printerData) {
    if (resource.phase === "disconnected") {
      return (
        <section class="rounded-box bg-base-100 p-5 shadow-sm" aria-live="polite">
          <p>Unable to connect; retrying automatically.</p>
        </section>
      );
    }
    return <p aria-live="polite" class="text-base-content/70">Connecting to printer monitor…</p>;
  }

  return (
    <div class="space-y-4">
      {resource.phase === "disconnected" && (
        <p role="alert" class="alert alert-warning">
          Showing stale printer data; reconnecting automatically. {resource.error.message}
        </p>
      )}
      {snapshot.warning && <p role="alert" class="alert alert-warning">{snapshot.warning}</p>}
      {printerData.length === 0 ? (
        <section class="rounded-box bg-base-100 p-5 shadow-sm"><p>No printers configured.</p></section>
      ) : <>
        <div class="hidden overflow-x-auto rounded-box bg-base-100 shadow-sm lg:block">
          <table class="table w-full" style={{ tableLayout: "fixed" }}>
            <colgroup>
              <col style={{ width: "20%" }} />
              <col style={{ width: "15%" }} />
              <col style={{ width: "20%" }} />
              <col style={{ width: "45%" }} />
            </colgroup>
            <thead><tr><th>Name</th><th>Status</th><th>Profile</th><th>Connection</th></tr></thead>
            <tbody>{printerData.map((printer) => <tr key={printer.name} class={flashClass(resource.printerFlashes, printer.name)}>
              <td style={{ overflowWrap: "anywhere" }}>{printer.name}</td>
              <td style={{ overflowWrap: "anywhere" }}>{titleCase(printer.availability)}</td>
              <td style={{ overflowWrap: "anywhere" }}>{printer.profile ?? "No profile"}</td>
              <td style={{ overflowWrap: "anywhere" }}>{connection(printer)}</td>
            </tr>)}</tbody>
          </table>
        </div>
        <div class="space-y-3 lg:hidden">
          {printerData.map((printer) => (
            <article key={printer.name} class={["rounded-box bg-base-100 p-5 shadow-sm", flashClass(resource.printerFlashes, printer.name)].join(" ").trim()}>
              {/* The label column takes exactly the width of its longest
                  label rather than half the card, so a value sits next to
                  what names it instead of across a gulf from it. The gap
                  does the separating. */}
              <dl class="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-2 text-sm">
                <dt class="font-medium text-base-content/70">Name</dt><dd>{printer.name}</dd>
                <dt class="font-medium text-base-content/70">Status</dt><dd>{titleCase(printer.availability)}</dd>
                <dt class="font-medium text-base-content/70">Profile</dt><dd>{printer.profile ?? "No profile"}</dd>
                <dt class="font-medium text-base-content/70">Connection</dt><dd>{connection(printer)}</dd>
              </dl>
            </article>
          ))}
        </div>
      </>}
      <p class="text-sm text-base-content/60">
        Last updated <time dateTime={snapshot.updated_at}>{timestampFormatter.format(new Date(snapshot.updated_at))}</time>
      </p>
    </div>
  );
}
