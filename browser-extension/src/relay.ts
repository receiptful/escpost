import { isPageRequest, type PageReply, type PageRequest, type WorkerReply } from "./protocol";

type RelayWindow = {
  location: { origin: string };
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  postMessage(message: PageReply, targetOrigin: string): void;
};

type Runtime = { sendMessage(message: unknown): Promise<unknown> };

export function installRelay(
  page: RelayWindow = window as unknown as RelayWindow,
  runtime: Runtime = chrome.runtime,
): void {
  page.addEventListener("message", (event) => {
    if (event.source !== page || !isReplyablePageMessage(event.data)) return;
    if (!isPageRequest(event.data)) {
      page.postMessage(protocolFailure(event.data.id), page.location.origin);
      return;
    }
    void forward(event.data, page, runtime);
  });
}

async function forward(request: PageRequest, page: RelayWindow, runtime: Runtime): Promise<void> {
  try {
    const reply = await runtime.sendMessage({ source: "escpost-relay", request });
    if (!isWorkerReply(reply)) return;
    page.postMessage({ source: "escpost-extension", id: request.id, ...reply }, page.location.origin);
  } catch {
    // The page SDK owns its extension-unavailable timeout when Chrome cannot
    // deliver a one-shot worker message.
  }
}

function isWorkerReply(value: unknown): value is WorkerReply {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const reply = value as { ok?: unknown; data?: unknown; error?: unknown };
  return reply.ok === true || (reply.ok === false && typeof reply.error === "object" && reply.error !== null);
}

function isReplyablePageMessage(value: unknown): value is { source: "escpost-page"; id: number } {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const request = value as { source?: unknown; id?: unknown };
  return request.source === "escpost-page" && typeof request.id === "number" && Number.isSafeInteger(request.id);
}

function protocolFailure(id: number): PageReply {
  return {
    source: "escpost-extension",
    id,
    ok: false,
    error: { code: "PROTOCOL_MISMATCH", message: "The page request does not match the ESCPost protocol." },
  };
}

if (typeof window !== "undefined" && typeof chrome !== "undefined") installRelay();
