import type { ComponentChildren } from "preact";
import logoDark from "../../assets/logo_dark.png";
import logoLight from "../../assets/logo_light.png";
import { useAppData } from "../../app/data";

function SummaryCard({ children, label }: { children: ComponentChildren; label: string }) {
  return (
    <section aria-label={label} class="rounded-box bg-base-100 p-5 text-center shadow-sm">
      <h2 class="text-left text-sm font-medium text-base-content/70">{label}</h2>
      {children}
    </section>
  );
}

export function OverviewPage() {
  const { printers, status } = useAppData();
  const inventory = printers.data?.printers;
  const connected = inventory?.filter((printer) => printer.availability === "connected").length;
  const unavailable = inventory?.filter((printer) => printer.availability === "unavailable").length;
  const virtual = status?.virtual_printer;
  const virtualState = !status ? "Checking…" : !virtual ? "Not running" : virtual.state === "receiving" ? "Receiving" : "Ready";

  return (
    <section aria-labelledby="overview-heading" class="mx-auto w-full max-w-7xl space-y-8 pt-6 lg:pt-10">
      <h1 id="overview-heading" class="sr-only">Overview</h1>
      <picture class="mx-auto block w-full max-w-lg">
        <source media="(prefers-color-scheme: dark)" srcSet={logoDark} />
        <img class="w-full rounded-box" src={logoLight} alt="ESCPost" />
      </picture>
      <div class="space-y-3">
        <div class="grid gap-4 md:grid-cols-3">
          <SummaryCard label="Jobs processed">
            <p class="mt-2 text-2xl font-bold">{status?.jobs_processed ?? "—"}</p>
          </SummaryCard>
          <SummaryCard label="Printers">
            {inventory ? (
              <>
                <p class="mt-2 text-2xl font-bold">{inventory.length} configured</p>
                {((connected ?? 0) > 0 || (unavailable ?? 0) > 0) && (
                  <div class="mt-4 flex flex-wrap justify-center gap-3 text-sm">
                    {(connected ?? 0) > 0 && <span class="badge badge-success">{connected} connected</span>}
                    {(unavailable ?? 0) > 0 && <span class="badge badge-warning">{unavailable} unavailable</span>}
                  </div>
                )}
              </>
            ) : (
              <p class="mt-2 text-lg font-semibold">{printers.phase === "loading" ? "Printer inventory loading…" : printers.error?.message ?? "Printer inventory is unavailable."}</p>
            )}
            {inventory && printers.error && <p role="alert" class="mt-4 text-warning">Showing cached printer data. {printers.error.message}</p>}
          </SummaryCard>
          <SummaryCard label="Virtual printer">
            <p class="mt-2 text-2xl font-bold">{virtualState}</p>
            <p class="mt-2 text-base-content/70">{virtual?.address ?? "No virtual printer is running."}</p>
          </SummaryCard>
        </div>
        {/* The file every printer command writes to, which the CLI prints on
            each of them. A footnote to the runtime facts rather than a fourth
            card: nothing in this app changes it, so it is stated and nothing
            more — no link, no edit affordance. */}
        {status?.config_path && (
          <p class="text-sm text-base-content/60">
            Configuration <span class="font-mono">{status.config_path}</span>
          </p>
        )}
      </div>
    </section>
  );
}
