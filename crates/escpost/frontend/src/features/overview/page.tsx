import type { ComponentChildren } from "preact";
import logoDark from "../../assets/logo_dark.png";
import logoLight from "../../assets/logo_light.png";
import { copyText } from "../../app/clipboard";
import { usePrinterInventory } from "../../app/printer-inventory-data";
import { useServerStatus } from "../../app/server-status-data";

function DashboardCard({ accessibleLabel, footer, href, label, status, title, value }: {
  accessibleLabel: string;
  footer?: ComponentChildren;
  href: string;
  label: string;
  status?: ComponentChildren;
  title: string;
  value: ComponentChildren;
}) {
  return (
    <section aria-label={accessibleLabel} class="rounded-box relative cursor-pointer bg-base-100 p-5 text-center shadow-sm transition-shadow hover:shadow-md focus-within:shadow-md">
      <a aria-label={`Open ${title}`} class="rounded-box absolute inset-0 z-10 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary" href={href} />
      <div class="flex min-h-6 items-center justify-between gap-3">
        <h2 class="shrink-0 text-left text-sm font-medium text-base-content/70">{title}</h2>
        {status}
      </div>
      <p class="mt-3 text-2xl font-bold">{value}</p>
      <p class="mt-1 text-sm text-base-content/70">{label}</p>
      {footer && <footer class="mt-4">{footer}</footer>}
    </section>
  );
}

function CopyableEndpointField({ copyLabel, groupLabel, label, value }: { copyLabel: string; groupLabel: string; label: string; value: string }) {
  return (
    <div role="group" aria-label={groupLabel} class="rounded-box flex items-center gap-2 border border-base-300 bg-base-200/50 px-3 py-2 text-left">
      <span class="shrink-0 text-xs font-medium text-base-content/60">{label}:</span>
      <code class="min-w-0 flex-1 truncate text-sm text-base-content">{value}</code>
      <button type="button" class="btn btn-ghost btn-xs relative z-20" aria-label={copyLabel} title={copyLabel} onClick={() => void copyText(value)}>Copy</button>
    </div>
  );
}

export function OverviewPage() {
  const inventoryResource = usePrinterInventory();
  const status = useServerStatus();
  const snapshot = status.snapshot;
  const inventory = inventoryResource.snapshot?.printers;
  const connected = inventory?.filter((printer) => printer.availability === "connected").length;
  const unavailable = inventory?.filter((printer) => printer.availability === "unavailable").length;
  const virtual = snapshot?.virtual_printer;
  const virtualState = !snapshot ? "Checking…" : !virtual ? "Not running" : virtual.state === "receiving" ? "Receiving" : "Ready";
  const virtualStateClass = !snapshot || !virtual ? "badge-ghost" : virtual.state === "receiving" ? "badge-info" : "badge-success";
  const virtualAddressSeparator = virtual?.address.lastIndexOf(":") ?? -1;
  const virtualAddressHost = virtual?.address.slice(0, virtualAddressSeparator);
  const virtualIp = virtualAddressHost?.startsWith("[") && virtualAddressHost.endsWith("]") ? virtualAddressHost.slice(1, -1) : virtualAddressHost;
  const virtualPort = virtual?.address.slice(virtualAddressSeparator + 1);
  const inventoryMessage = !inventory
    ? inventoryResource.phase === "checking" ? "Connecting to printer monitor…" : "Unable to connect; retrying automatically."
    : inventoryResource.phase === "disconnected" ? "Showing stale printer data; reconnecting automatically." : null;

  return (
    <section aria-labelledby="overview-heading" class="mx-auto w-full max-w-7xl space-y-8 pt-6 lg:pt-10">
      <h1 id="overview-heading" class="sr-only">Overview</h1>
      <picture class="mx-auto block w-full max-w-lg">
        <source media="(prefers-color-scheme: dark)" srcSet={logoDark} />
        <img class="w-full rounded-box" src={logoLight} alt="ESCPost" />
      </picture>
      <div class="mx-auto w-full max-w-[54rem] space-y-3">
        <div class="grid gap-4 xl:grid-cols-2">
          <DashboardCard
            accessibleLabel="Configured printers"
            href="/printers"
            label="Configured printers"
            title="Printers"
            value={inventory?.length ?? "—"}
            status={inventory && ((connected ?? 0) > 0 || (unavailable ?? 0) > 0) ? (
              <div class="flex min-w-0 flex-wrap justify-end gap-2">
                {(connected ?? 0) > 0 && <span class="badge badge-success">{connected} connected</span>}
                {(unavailable ?? 0) > 0 && <span class="badge badge-warning">{unavailable} unavailable</span>}
              </div>
            ) : undefined}
          />
          <DashboardCard
            accessibleLabel="Virtual printer"
            href="/jobs"
            label="jobs processed this session"
            title="Virtual printer"
            value={snapshot?.jobs_processed ?? "—"}
            status={<span class={`badge ${virtualStateClass}`}>{virtualState}</span>}
            footer={virtualIp && virtualPort ? (
              <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
                <CopyableEndpointField copyLabel="Copy virtual printer IP" groupLabel="Virtual printer IP" label="IP" value={virtualIp} />
                <CopyableEndpointField copyLabel="Copy virtual printer port" groupLabel="Virtual printer port" label="Port" value={virtualPort} />
              </div>
            ) : (
              <p class="text-base-content/70">Virtual printer is disabled.</p>
            )}
          />
        </div>
        {inventoryMessage && (
          <p role={inventoryResource.phase === "disconnected" ? "alert" : undefined} class={`text-sm ${inventoryResource.phase === "disconnected" ? "text-warning" : "text-base-content/70"}`}>
            {inventoryMessage}
          </p>
        )}
        {inventoryResource.snapshot?.warning && <p role="alert" class="text-sm text-warning">{inventoryResource.snapshot.warning}</p>}
        {/* The file every printer command writes to, which the CLI prints on
            each of them. A footnote to the runtime facts rather than a fourth
            card: nothing in this app changes it, so it is stated and nothing
            more — no link, no edit affordance. */}
        {snapshot?.config_path && (
          <p class="text-sm text-base-content/60">
            Configuration <span class="font-mono">{snapshot.config_path}</span>
          </p>
        )}
      </div>
    </section>
  );
}
