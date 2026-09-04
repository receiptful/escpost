import { DaemonClient, type InventoryStreamCallbacks } from "../daemon";
import { ChromeOriginGrants } from "./grants";
import { installInventoryStreams, inventoryPortName } from "../inventory-stream";
import { handleRequest, type RequestDependencies } from "../messages";

type ParentWindow = { postMessage(message: unknown, targetOrigin: string): void };
type BridgeWindow = {
  parent: ParentWindow;
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
};
type BridgeDependencies = RequestDependencies & {
  daemon: RequestDependencies["daemon"] & {
    openInventoryStream(callbacks: InventoryStreamCallbacks, signal: AbortSignal): Promise<void>;
  };
};

type Listener<T> = { addListener(listener: T): void };

class BridgePort {
  readonly name = inventoryPortName;
  readonly sender: { url: string };
  private message: ((message: unknown) => void) | undefined;
  private disconnected: (() => void) | undefined;

  readonly onMessage: Listener<(message: unknown) => void> = {
    addListener: (listener) => { this.message = listener; },
  };
  readonly onDisconnect: Listener<() => void> = {
    addListener: (listener) => { this.disconnected = listener; },
  };

  constructor(private readonly page: BridgeWindow, private readonly origin: string) {
    this.sender = { url: origin };
  }

  postMessage(message: unknown): void {
    if (!isRecord(message)) return;
    this.page.parent.postMessage({ ...message, source: "escpost-extension" }, this.origin);
  }

  receive(message: unknown): void { this.message?.(message); }
  disconnect(): void { this.disconnected?.(); }
}

export function installChromeBridge(page: BridgeWindow, deps: BridgeDependencies): void {
  let connect: ((port: BridgePort) => void) | undefined;
  installInventoryStreams(
    { onConnect: { addListener(listener) { connect = listener; } } },
    deps,
  );
  let ownerOrigin: string | undefined;
  let port: BridgePort | undefined;

  page.addEventListener("message", (event) => {
    if (event.source !== page.parent || !isWebOrigin(event.origin)) return;
    if (ownerOrigin !== undefined && ownerOrigin !== event.origin) return;
    ownerOrigin ??= event.origin;

    if (isReplyableRequest(event.data)) {
      void handleRequest(event.data, ownerOrigin, deps).then((reply) => {
        page.parent.postMessage({ source: "escpost-extension", id: event.data.id, ...reply }, ownerOrigin!);
      });
      return;
    }

    if (!isStreamRequest(event.data)) return;
    if (port === undefined) {
      port = new BridgePort(page, ownerOrigin);
      connect?.(port);
    }
    port.receive(event.data.kind === "subscribe"
      ? { kind: "subscribe", subscriptionId: event.data.subscriptionId, protocol: event.data.protocol }
      : { kind: "unsubscribe", subscriptionId: event.data.subscriptionId });
  });
}

function isReplyableRequest(value: unknown): value is { source: "escpost-page"; id: number } {
  return isRecord(value) && value.source === "escpost-page" && Number.isSafeInteger(value.id);
}

function isStreamRequest(value: unknown): value is {
  source: "escpost-page";
  kind: "subscribe" | "unsubscribe";
  subscriptionId: unknown;
  protocol?: unknown;
} {
  return isRecord(value)
    && value.source === "escpost-page"
    && Object.hasOwn(value, "subscriptionId")
    && (value.kind === "subscribe" || value.kind === "unsubscribe");
}

function isWebOrigin(origin: string): boolean {
  try {
    const url = new URL(origin);
    return (url.protocol === "http:" || url.protocol === "https:") && url.origin === origin;
  } catch {
    return false;
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

if (typeof window !== "undefined" && window.parent !== window) {
  const daemon = new DaemonClient();
  const grants = new ChromeOriginGrants(chrome.storage.local, chrome.storage.onChanged);
  installChromeBridge(window, { grants, daemon });
}
