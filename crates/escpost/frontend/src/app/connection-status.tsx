import { useAppData } from "./data";

export function ConnectionStatus({ compact = false }: { compact?: boolean }) {
  const { connection, statusError } = useAppData();
  const label = connection === "ready" ? "Ready" : connection === "disconnected" ? "Disconnected" : "Checking…";
  return (
    <section
      aria-label="Server status"
      aria-live="polite"
      aria-atomic="true"
      class={compact ? "rounded-box bg-base-200 px-3 py-2 text-xs" : "mt-auto rounded-box bg-base-200 p-4 text-sm"}
      role="status"
    >
      <p class="font-medium">Server status</p>
      <p class="mt-1 text-base-content/70">{label}</p>
      {statusError && <p role="alert" class="mt-2 text-warning">Status check unavailable: {statusError.message}</p>}
    </section>
  );
}
