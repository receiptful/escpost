import type { PrintersResponse } from "./types";

export function openPrinterInventoryStream(callbacks: {
  onSnapshot: (snapshot: PrintersResponse) => void;
  onError: (error: Error) => void;
}): () => void {
  const source = new EventSource("/api/printers/list/events");
  source.addEventListener("message", (event) => {
    try {
      callbacks.onSnapshot(JSON.parse((event as MessageEvent<string>).data) as PrintersResponse);
    } catch {
      callbacks.onError(new Error("The server sent an invalid printer inventory."));
    }
  });
  source.addEventListener("error", () => {
    callbacks.onError(new Error("Printer monitoring disconnected; retrying automatically."));
  });
  return () => source.close();
}
