import { PrinterList } from "./printer-list";

export function PrintersPage() {
  return (
    <section aria-labelledby="printers-heading" class="space-y-6">
      <h1 id="printers-heading" class="sr-only">Printers</h1>
      <PrinterList />
    </section>
  );
}
