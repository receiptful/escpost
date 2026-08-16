import { useAppData } from "../../app/data";
import { PrinterList } from "./printer-list";

export function PrintersPage() {
  const { printers, refreshPrinters } = useAppData();
  return (
    <section aria-labelledby="printers-heading" class="space-y-6">
      <div class="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p class="text-sm font-semibold text-primary">Workbench</p>
          <h1 id="printers-heading" class="mt-1 text-3xl font-bold">Printers</h1>
        </div>
        <button class="btn btn-primary" type="button" onClick={() => void refreshPrinters()} disabled={printers.phase === "loading" || printers.phase === "refreshing"}>
          Refresh
        </button>
      </div>
      <PrinterList />
    </section>
  );
}
