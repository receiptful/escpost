import type { ServerStatusSnapshot, VirtualPrinterStatus } from "./types";

export type ServerStatusHandlers = {
  onStatus: (snapshot: ServerStatusSnapshot) => void;
  onError: (error: Error) => void;
};

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isVirtualPrinterStatus(value: unknown): value is VirtualPrinterStatus {
  return isObject(value)
    && (value.state === "ready" || value.state === "receiving")
    && typeof value.address === "string";
}

function isServerStatusSnapshot(value: unknown): value is ServerStatusSnapshot {
  return isObject(value)
    && (value.virtual_printer === null || isVirtualPrinterStatus(value.virtual_printer))
    && typeof value.jobs_processed === "number"
    && Number.isSafeInteger(value.jobs_processed)
    && value.jobs_processed >= 0
    && typeof value.config_path === "string";
}

export function openServerStatusStream(handlers: ServerStatusHandlers): () => void {
  const source = new EventSource("/api/status/events");
  source.addEventListener("status", (event) => {
    try {
      const value: unknown = JSON.parse((event as MessageEvent<string>).data);
      if (!isServerStatusSnapshot(value)) {
        throw new Error("The server returned invalid status data.");
      }
      handlers.onStatus(value);
    } catch (error) {
      handlers.onError(error instanceof Error ? error : new Error("The server returned invalid status data."));
    }
  });
  source.addEventListener("error", () => {
    handlers.onError(new Error("Unable to reach the ESCPost server."));
  });
  return () => source.close();
}
