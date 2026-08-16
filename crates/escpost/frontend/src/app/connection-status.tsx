import { useAppData } from "./data";

export function ConnectionStatus() {
  const { connection } = useAppData();
  const label = connection === "ready" ? "Ready" : connection === "disconnected" ? "Disconnected" : "Checking…";
  return (
    <section aria-label="Connection status" class="mt-auto rounded-box bg-base-200 p-4 text-sm">
      <p class="font-medium">Connection</p>
      <p class="mt-1 text-base-content/70">{label}</p>
    </section>
  );
}
