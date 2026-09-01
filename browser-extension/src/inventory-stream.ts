import type { InventoryStreamCallbacks, WirePrinterInventory } from "./daemon";
import { isDaemonOrigin, originPattern } from "./registration";

export const inventoryPortName = "escpost-printers";

type StreamPort = {
  name: string;
  sender?: { url?: string };
  onMessage: { addListener(listener: (message: unknown) => void): void };
  onDisconnect: { addListener(listener: () => void): void };
  postMessage(message: unknown): void;
};

type StreamRuntime = {
  onConnect: { addListener(listener: (port: StreamPort) => void): void };
};

type InventoryDaemon = {
  openInventoryStream(callbacks: InventoryStreamCallbacks, signal: AbortSignal): Promise<void>;
};

export type InventoryStreamDependencies = {
  permissions: { contains(details: { origins: string[] }): Promise<boolean> };
  daemon: InventoryDaemon;
};

const reconnectDelays = [150, 300, 600, 1_000] as const;
const disconnectedError = {
  code: "DAEMON_UNAVAILABLE",
  message: "The local ESCPost daemon inventory stream disconnected.",
} as const;
const deniedError = {
  code: "ORIGIN_NOT_GRANTED",
  message: "This page origin is not granted access to ESCPost.",
} as const;

export function installInventoryStreams(runtime: StreamRuntime, deps: InventoryStreamDependencies): void {
  runtime.onConnect.addListener((port) => {
    const pattern = trustedSenderPattern(port);
    if (pattern === null) return;
    ownPortStream(port, pattern, deps);
  });
}

function ownPortStream(port: StreamPort, pattern: string, deps: InventoryStreamDependencies): void {
  const subscriptions = new Set<number>();
  const deniedSubscriptions = new Set<number>();
  let authorization: Promise<boolean> | undefined;
  let stream: AbortController | undefined;
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
  let reconnectIndex = 0;
  let disconnected = false;

  const stopOwnedWork = () => {
    if (reconnectTimer !== undefined) {
      clearTimeout(reconnectTimer);
      reconnectTimer = undefined;
    }
    const ownedStream = stream;
    stream = undefined;
    ownedStream?.abort();
  };

  const close = () => {
    if (disconnected) return;
    disconnected = true;
    subscriptions.clear();
    deniedSubscriptions.clear();
    stopOwnedWork();
  };

  const post = (message: unknown) => {
    if (disconnected) return;
    try {
      port.postMessage(message);
    } catch {
      close();
    }
  };

  const fanSnapshot = (snapshot: WirePrinterInventory) => {
    for (const subscriptionId of subscriptions) {
      post({ kind: "snapshot", subscriptionId, data: snapshot });
      if (disconnected) return;
    }
  };

  const fanFailure = (error: typeof disconnectedError | typeof deniedError) => {
    for (const subscriptionId of subscriptions) {
      post({ kind: "failure", subscriptionId, error });
      if (disconnected) return;
    }
  };

  const startAttempt = () => {
    if (disconnected || subscriptions.size === 0 || stream !== undefined || reconnectTimer !== undefined) return;
    const attempt = new AbortController();
    stream = attempt;
    const callbacks: InventoryStreamCallbacks = {
      onSnapshot(snapshot) {
        if (disconnected || stream !== attempt || attempt.signal.aborted || subscriptions.size === 0) return;
        reconnectIndex = 0;
        fanSnapshot(snapshot);
      },
      onError() {
        // DaemonClient reports terminal transport errors before returning. The
        // completion path below emits exactly one failure for that disconnect.
      },
    };
    void (async () => {
      try {
        await deps.daemon.openInventoryStream(callbacks, attempt.signal);
      } catch {
        // A thrown transport failure has the same reconnect contract as a
        // callback-reported failure or a clean but unexpected end of stream.
      }
      if (disconnected || stream !== attempt || attempt.signal.aborted || subscriptions.size === 0) return;
      stream = undefined;
      fanFailure(disconnectedError);
      if (disconnected || subscriptions.size === 0) return;
      const delay = reconnectDelays[Math.min(reconnectIndex, reconnectDelays.length - 1)] ?? 1_000;
      reconnectIndex += 1;
      reconnectTimer = setTimeout(() => {
        reconnectTimer = undefined;
        void authorizeAndStart();
      }, delay);
    })();
  };

  const authorizeAndStart = async () => {
    const decision = authorization ??= granted(pattern, deps);
    const authorized = await decision;
    if (authorization === decision) authorization = undefined;
    if (disconnected || subscriptions.size === 0) return;
    if (!authorized) {
      for (const subscriptionId of subscriptions) {
        if (deniedSubscriptions.has(subscriptionId)) continue;
        deniedSubscriptions.add(subscriptionId);
        post({ kind: "failure", subscriptionId, error: deniedError });
        if (disconnected) return;
      }
      return;
    }
    deniedSubscriptions.clear();
    startAttempt();
  };

  port.onMessage.addListener((message) => {
    if (disconnected) return;
    if (isSubscribe(message)) {
      subscriptions.add(message.subscriptionId);
      void authorizeAndStart();
      return;
    }
    if (!isUnsubscribe(message)) return;
    subscriptions.delete(message.subscriptionId);
    deniedSubscriptions.delete(message.subscriptionId);
    if (subscriptions.size === 0) stopOwnedWork();
  });
  port.onDisconnect.addListener(close);
}

async function granted(pattern: string, deps: InventoryStreamDependencies): Promise<boolean> {
  try {
    return await deps.permissions.contains({ origins: [pattern] });
  } catch {
    return false;
  }
}

function trustedSenderPattern(port: StreamPort): string | null {
  if (port.name !== inventoryPortName || port.sender?.url === undefined) return null;
  try {
    const url = new URL(port.sender.url);
    if ((url.protocol !== "http:" && url.protocol !== "https:")
      || url.origin === "null"
      || url.hostname.length === 0
      || url.hostname.includes("*")
      || isDaemonOrigin(url.origin)) {
      return null;
    }
    return originPattern(url.origin);
  } catch {
    return null;
  }
}

function isSubscribe(value: unknown): value is { kind: "subscribe"; subscriptionId: number; protocol: 1 } {
  return isRecord(value)
    && hasExactKeys(value, ["kind", "subscriptionId", "protocol"])
    && value.kind === "subscribe"
    && value.protocol === 1
    && isSubscriptionId(value.subscriptionId);
}

function isUnsubscribe(value: unknown): value is { kind: "unsubscribe"; subscriptionId: number } {
  return isRecord(value)
    && hasExactKeys(value, ["kind", "subscriptionId"])
    && value.kind === "unsubscribe"
    && isSubscriptionId(value.subscriptionId);
}

function isSubscriptionId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function hasExactKeys(value: Record<string, unknown>, keys: string[]): boolean {
  return Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
