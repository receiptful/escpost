import { EXT_SOURCE, PAGE_SOURCE, type WorkerResponse } from "./protocol";

/**
 * Set on <html> so the popup can tell a granted page that is already relaying
 * from one that was loaded before the grant and needs a reload. Nothing else
 * reads it, and the page is free to remove it: the worst that follows is being
 * offered a reload that was not needed.
 */
document.documentElement.setAttribute("data-escpost-relay", "1");

window.addEventListener("message", (event: MessageEvent) => {
  if (event.source !== window) return;
  const message = event.data as { source?: string; id?: number; op?: string; payload?: unknown; protocol?: number };
  if (message?.source !== PAGE_SOURCE || typeof message.id !== "number") return;

  chrome.runtime.sendMessage({ op: message.op, payload: message.payload, protocol: message.protocol }, (response: WorkerResponse | undefined) => {
    const failure = chrome.runtime.lastError;
    const body: Record<string, unknown> = failure
      ? { ok: false, error: { code: "PRINT_FAILED", message: failure.message } }
      : (response ?? { ok: false, error: { code: "PRINT_FAILED", message: "The extension did not respond." } });
    window.postMessage({ source: EXT_SOURCE, id: message.id, ...body }, "*");
  });
});
