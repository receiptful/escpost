import { extensionProtocolVersion, isPageRequest, type PageReply, type PageRequest, type WorkerReply } from "./protocol";

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
    const origin = currentOrigin(page);
    if (origin === null || event.source !== page || event.origin !== origin || !isReplyablePageMessage(event.data)) return;
    if (!isPageRequest(event.data) || event.data.protocol !== extensionProtocolVersion) {
      page.postMessage(protocolFailure(event.data.id), origin);
      return;
    }
    void forward(event.data, page, runtime, origin);
  });
}

async function forward(request: PageRequest, page: RelayWindow, runtime: Runtime, origin: string): Promise<void> {
  const respond = guardedResponse(page, request.id, origin);
  try {
    const reply = await runtime.sendMessage({ source: "escpost-relay", request });
    respond(isWorkerReply(reply) ? reply : protocolFailureReply());
  } catch {
    respond(failure("EXTENSION_UNAVAILABLE", "The extension worker could not receive the page request."));
  }
}

function currentOrigin(page: RelayWindow): string | null {
  try {
    const url = new URL(page.location.origin);
    return (url.protocol === "http:" || url.protocol === "https:") && url.origin === page.location.origin ? url.origin : null;
  } catch {
    return null;
  }
}

function isWorkerReply(value: unknown): value is WorkerReply {
  if (!isRecord(value) || !Object.hasOwn(value, "ok")) return false;
  if (value.ok === true) return hasExactOwnKeys(value, ["ok", "data"]);
  return value.ok === false
    && hasExactOwnKeys(value, ["ok", "error"])
    && isSerializedError(value.error);
}

function isSerializedError(value: unknown): boolean {
  if (!isRecord(value) || !hasExactOwnKeys(value, ["code", "message"])) return false;
  return typeof value.code === "string"
    && knownErrorCodes.has(value.code)
    && typeof value.message === "string";
}

function hasExactOwnKeys(value: Record<string, unknown>, keys: string[]): boolean {
  return Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const knownErrorCodes: ReadonlySet<string> = new Set([
  "EXTENSION_UNAVAILABLE",
  "ORIGIN_NOT_GRANTED",
  "DAEMON_UNAVAILABLE",
  "PRINTER_NOT_FOUND",
  "PRINT_FAILED",
  "PROTOCOL_MISMATCH",
]);

function isReplyablePageMessage(value: unknown): value is { source: "escpost-page"; id: number } {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const request = value as { source?: unknown; id?: unknown };
  return request.source === "escpost-page" && typeof request.id === "number" && Number.isSafeInteger(request.id);
}

function protocolFailure(id: number): PageReply {
  return { source: "escpost-extension", id, ...protocolFailureReply() };
}

function protocolFailureReply(): WorkerReply {
  return failure("PROTOCOL_MISMATCH", "The page request does not match the ESCPost protocol.");
}

function failure(code: "EXTENSION_UNAVAILABLE" | "PROTOCOL_MISMATCH", message: string): WorkerReply {
  return { ok: false, error: { code, message } };
}

function guardedResponse(page: RelayWindow, id: number, origin: string): (reply: WorkerReply) => void {
  let responded = false;
  return (reply) => {
    if (responded || currentOrigin(page) !== origin) return;
    responded = true;
    page.postMessage(
      reply.ok === true
        ? { source: "escpost-extension", id, ok: true, data: reply.data }
        : { source: "escpost-extension", id, ok: false, error: reply.error },
      origin,
    );
  };
}

if (typeof window !== "undefined" && typeof chrome !== "undefined") installRelay();
