import { useAppData } from "../../app/data";

function StatCard({ label, value }: { label: string; value: string | number }) {
  return (
    <section class="rounded-box bg-base-100 p-5 shadow-sm">
      <p class="text-sm font-medium text-base-content/70">{label}</p>
      <p class="mt-2 text-2xl font-bold">{value}</p>
    </section>
  );
}

export function OverviewPage() {
  const { connection, printers, status } = useAppData();
  const inventory = printers.data?.printers;
  const connected = inventory?.filter((printer) => printer.availability === "connected").length;
  const unavailable = inventory?.filter((printer) => printer.availability === "unavailable").length;
  const virtual = status?.virtual_printer;
  const virtualState = !status ? "Checking…" : !virtual ? "Not running" : virtual.state === "receiving" ? "Receiving" : "Ready";
  const serverState = connection === "ready" ? "Ready" : connection === "disconnected" ? "Disconnected" : "Checking…";

  return (
    <section aria-labelledby="overview-heading" class="space-y-6">
      <div>
        <p class="text-sm font-semibold text-primary">Workbench</p>
        <h1 id="overview-heading" class="mt-1 text-3xl font-bold">Overview</h1>
      </div>
      <div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <StatCard label="Server" value={serverState} />
        <StatCard label="Virtual printer" value={virtualState} />
        <StatCard label="Session jobs processed" value={status?.jobs_processed ?? "—"} />
        <StatCard label="Printers" value={inventory ? `${inventory.length} configured` : printers.phase === "loading" ? "Loading…" : "Unknown"} />
      </div>
      <section aria-label="Printer availability" class="rounded-box bg-base-100 p-5 shadow-sm">
        <h2 class="text-lg font-semibold">Printer availability</h2>
        {inventory ? (
          <div class="mt-4 flex flex-wrap gap-3 text-sm">
            <span class="badge badge-success">{connected} connected</span>
            <span class="badge badge-warning">{unavailable} unavailable</span>
          </div>
        ) : (
          <p class="mt-4 text-base-content/70">{printers.phase === "loading" ? "Printer inventory loading…" : printers.error?.message ?? "Printer inventory is unavailable."}</p>
        )}
        {inventory && printers.error && <p role="alert" class="mt-4 text-warning">Showing cached printer data. {printers.error.message}</p>}
      </section>
      <section aria-label="Virtual printer details" class="rounded-box bg-base-100 p-5 shadow-sm">
        <h2 class="text-lg font-semibold">Virtual printer</h2>
        <p class="mt-2 text-base-content/70">{virtual?.address ?? "No virtual printer is running."}</p>
      </section>
    </section>
  );
}
