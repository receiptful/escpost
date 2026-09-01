import { extensionProtocolVersion, isPageRequest, type PageReply, type PageRequest, type WorkerReply } from "./protocol";

type RelayWindow = {
  location: { origin: string };
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  postMessage(message: PageReply | PageStreamMessage, targetOrigin: string): void;
};

type RuntimePort = {
  onMessage: { addListener(listener: (message: unknown) => void): void };
  onDisconnect: { addListener(listener: () => void): void };
  postMessage(message: unknown): void;
  disconnect(): void;
};

type Runtime = {
  sendMessage(message: unknown): Promise<unknown>;
  connect?(details: { name: string }): RuntimePort;
};

type PageStreamMessage =
  | { source: "escpost-extension"; kind: "snapshot"; subscriptionId: number; data: unknown }
  | { source: "escpost-extension"; kind: "failure"; subscriptionId: number; error: { code: string; message: string } };

export function installRelay(
  page: RelayWindow = window as unknown as RelayWindow,
  runtime: Runtime = chrome.runtime,
): void {
  const streams = createStreamRelay(page, runtime);
  page.addEventListener("message", (event) => {
    const origin = currentOrigin(page);
    if (origin === null || event.source !== page || event.origin !== origin) return;
    if (isPageSubscription(event.data)) {
      streams.subscribe(event.data.subscriptionId, origin);
      return;
    }
    if (isPageUnsubscribe(event.data)) {
      streams.unsubscribe(event.data.subscriptionId);
      return;
    }
    if (isMismatchedPageSubscription(event.data)) {
      page.postMessage(streamProtocolFailure(event.data.subscriptionId), origin);
      return;
    }
    if (!isReplyablePageMessage(event.data)) return;
    if (!isPageRequest(event.data) || event.data.protocol !== extensionProtocolVersion) {
      page.postMessage(protocolFailure(event.data.id), origin);
      return;
    }
    void forward(event.data, page, runtime, origin);
  });
}

function createStreamRelay(page: RelayWindow, runtime: Runtime) {
  const subscriptions = new Set<number>();
  let port: RuntimePort | undefined;
  let ownerOrigin: string | undefined;
  let reconnectQueued = false;

  const closePort = () => {
    const ownedPort = port;
    port = undefined;
    if (ownedPort === undefined) return;
    try {
      ownedPort.disconnect();
    } catch {
      // The runtime already closed the port.
    }
  };

  const closeDocument = () => {
    subscriptions.clear();
    ownerOrigin = undefined;
    closePort();
  };

  const postFailure = (subscriptionId: number, code: "EXTENSION_UNAVAILABLE", message: string) => {
    const origin = ownerOrigin;
    if (origin === undefined || currentOrigin(page) !== origin || !subscriptions.has(subscriptionId)) return;
    page.postMessage({ source: "escpost-extension", kind: "failure", subscriptionId, error: { code, message } }, origin);
  };

  const queueReconnect = () => {
    if (reconnectQueued || subscriptions.size === 0 || ownerOrigin === undefined) return;
    reconnectQueued = true;
    queueMicrotask(() => {
      reconnectQueued = false;
      if (port === undefined && subscriptions.size > 0 && ownerOrigin !== undefined) openPort();
    });
  };

  const losePort = (lostPort: RuntimePort) => {
    if (port !== lostPort) return;
    port = undefined;
    queueReconnect();
  };

  const postToPort = (ownedPort: RuntimePort, message: unknown): boolean => {
    if (port !== ownedPort) return false;
    try {
      ownedPort.postMessage(message);
      return true;
    } catch {
      losePort(ownedPort);
      return false;
    }
  };

  const receive = (ownedPort: RuntimePort, message: unknown) => {
    if (port !== ownedPort) return;
    const origin = ownerOrigin;
    if (origin === undefined || currentOrigin(page) !== origin) {
      closeDocument();
      return;
    }
    if (!isWorkerStreamMessage(message) || !subscriptions.has(message.subscriptionId)) return;
    page.postMessage(
      message.kind === "snapshot"
        ? { source: "escpost-extension", kind: "snapshot", subscriptionId: message.subscriptionId, data: message.data }
        : { source: "escpost-extension", kind: "failure", subscriptionId: message.subscriptionId, error: message.error },
      origin,
    );
  };

  function openPort(): void {
    if (port !== undefined || subscriptions.size === 0) return;
    if (runtime.connect === undefined) {
      for (const subscriptionId of subscriptions) {
        postFailure(subscriptionId, "EXTENSION_UNAVAILABLE", "The extension worker stream is unavailable.");
      }
      return;
    }
    let opened: RuntimePort;
    try {
      opened = runtime.connect({ name: "escpost-printers" });
    } catch {
      for (const subscriptionId of subscriptions) {
        postFailure(subscriptionId, "EXTENSION_UNAVAILABLE", "The extension worker stream is unavailable.");
      }
      return;
    }
    port = opened;
    opened.onMessage.addListener((message) => receive(opened, message));
    opened.onDisconnect.addListener(() => losePort(opened));
    for (const subscriptionId of subscriptions) {
      if (!postToPort(opened, { kind: "subscribe", subscriptionId, protocol: extensionProtocolVersion })) return;
    }
  }

  return {
    subscribe(subscriptionId: number, origin: string) {
      if (ownerOrigin !== undefined && ownerOrigin !== origin) return;
      ownerOrigin ??= origin;
      if (subscriptions.has(subscriptionId)) return;
      subscriptions.add(subscriptionId);
      const ownedPort = port;
      if (ownedPort === undefined) {
        openPort();
      } else {
        postToPort(ownedPort, { kind: "subscribe", subscriptionId, protocol: extensionProtocolVersion });
      }
    },
    unsubscribe(subscriptionId: number) {
      if (!subscriptions.delete(subscriptionId)) return;
      const ownedPort = port;
      if (ownedPort !== undefined) postToPort(ownedPort, { kind: "unsubscribe", subscriptionId });
      if (subscriptions.size === 0) {
        ownerOrigin = undefined;
        closePort();
      }
    },
  };
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

function isWorkerStreamMessage(value: unknown): value is
  | { kind: "snapshot"; subscriptionId: number; data: unknown }
  | { kind: "failure"; subscriptionId: number; error: { code: string; message: string } } {
  if (!isRecord(value) || !isSubscriptionId(value.subscriptionId)) return false;
  if (value.kind === "snapshot") return hasExactOwnKeys(value, ["kind", "subscriptionId", "data"]);
  return value.kind === "failure"
    && hasExactOwnKeys(value, ["kind", "subscriptionId", "error"])
    && isSerializedError(value.error);
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

function isPageSubscription(value: unknown): value is {
  source: "escpost-page";
  kind: "subscribe";
  subscriptionId: number;
  op: "printers.events";
  protocol: 1;
} {
  return isRecord(value)
    && hasExactOwnKeys(value, ["source", "kind", "subscriptionId", "op", "protocol"])
    && value.source === "escpost-page"
    && value.kind === "subscribe"
    && isSubscriptionId(value.subscriptionId)
    && value.op === "printers.events"
    && value.protocol === extensionProtocolVersion;
}

function isMismatchedPageSubscription(value: unknown): value is { subscriptionId: number } {
  return isRecord(value)
    && hasExactOwnKeys(value, ["source", "kind", "subscriptionId", "op", "protocol"])
    && value.source === "escpost-page"
    && value.kind === "subscribe"
    && isSubscriptionId(value.subscriptionId)
    && value.op === "printers.events"
    && value.protocol !== extensionProtocolVersion;
}

function isPageUnsubscribe(value: unknown): value is {
  source: "escpost-page";
  kind: "unsubscribe";
  subscriptionId: number;
} {
  return isRecord(value)
    && hasExactOwnKeys(value, ["source", "kind", "subscriptionId"])
    && value.source === "escpost-page"
    && value.kind === "unsubscribe"
    && isSubscriptionId(value.subscriptionId);
}

function isSubscriptionId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function protocolFailure(id: number): PageReply {
  return { source: "escpost-extension", id, ...protocolFailureReply() };
}

function streamProtocolFailure(subscriptionId: number): PageStreamMessage {
  return {
    source: "escpost-extension",
    kind: "failure",
    subscriptionId,
    error: { code: "PROTOCOL_MISMATCH", message: "The subscription request does not match the ESCPost protocol." },
  };
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
