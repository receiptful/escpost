import { useAppData } from "./data";

export function ConnectionStatus({ compact = false }: { compact?: boolean }) {
  const { connection, statusError } = useAppData();
  const label = connection === "ready" ? "Ready" : connection === "disconnected" ? "Disconnected" : "Checking…";
  return (
    <section
      aria-label="Connection status"
      aria-live={compact ? "polite" : undefined}
      aria-atomic={compact ? "true" : undefined}
      class={compact ? "rounded-box bg-base-200 px-3 py-2 text-xs" : "mt-auto rounded-box bg-base-200 p-4 text-sm"}
      role={compact ? "status" : undefined}
    >
      <p class="font-medium">Connection</p>
      <p class="mt-1 text-base-content/70">{label}</p>
      {statusError && <p role="alert" class="mt-2 text-warning">Status check unavailable: {statusError.message}</p>}
    </section>
  );
}
