import type { Printer, UsbConnection } from "../../api/types";
import { useAppData } from "../../app/data";

function titleCase(value: string) {
  return `${value[0]?.toUpperCase()}${value.slice(1)}`;
}

function usbHex(value: number) {
  return value.toString(16).padStart(4, "0");
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

export function PrinterList() {
  const { printers, refreshPrinters } = useAppData();
  const printerData = printers.data?.printers;

  if (!printerData) {
    if (printers.phase === "error") {
      return (
        <section class="rounded-box bg-base-100 p-5 shadow-sm" aria-live="polite">
          <p>{printers.error?.message ?? "Unable to load printer inventory."}</p>
          <button class="btn btn-primary mt-4" type="button" onClick={() => void refreshPrinters()}>Retry</button>
        </section>
      );
    }
    return <p aria-live="polite" class="text-base-content/70">Loading printers…</p>;
  }

  return (
    <div class="space-y-4">
      {printers.phase === "refreshing" && <p aria-live="polite" class="text-sm text-base-content/70">Refreshing printers…</p>}
      {printers.error && (
        <p role="alert" class="alert alert-warning">
          Showing cached printer data. {printers.error.message}
        </p>
      )}
      {printerData.length === 0 ? (
        <section class="rounded-box bg-base-100 p-5 shadow-sm"><p>No printers configured.</p></section>
      ) : <>
        <div class="hidden overflow-x-auto rounded-box bg-base-100 shadow-sm lg:block">
          <table class="table">
            <thead><tr><th>Name</th><th>Status</th><th>Transport</th><th>Profile</th><th>Connection</th></tr></thead>
            <tbody>{printerData.map((printer) => <tr key={printer.name}>
              <td>{printer.name}</td>
              <td>{titleCase(printer.availability)}</td>
              <td>{titleCase(printer.transport)}</td>
              <td>{printer.profile ?? "No profile"}</td>
              <td>{connectionFacts(printer)}</td>
            </tr>)}</tbody>
          </table>
        </div>
        <div class="space-y-3 lg:hidden">
          {printerData.map((printer) => (
            <article key={printer.name} class="rounded-box bg-base-100 p-5 shadow-sm">
              <dl class="grid grid-cols-2 gap-3 text-sm">
                <dt class="font-medium text-base-content/70">Name</dt><dd>{printer.name}</dd>
                <dt class="font-medium text-base-content/70">Status</dt><dd>{titleCase(printer.availability)}</dd>
                <dt class="font-medium text-base-content/70">Transport</dt><dd>{titleCase(printer.transport)}</dd>
                <dt class="font-medium text-base-content/70">Profile</dt><dd>{printer.profile ?? "No profile"}</dd>
                <dt class="font-medium text-base-content/70">Connection</dt><dd>{connectionFacts(printer)}</dd>
              </dl>
            </article>
          ))}
        </div>
      </>}
    </div>
  );
}
