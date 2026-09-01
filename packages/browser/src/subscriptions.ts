import { EscpostError, fromSerializedError } from "./errors";
import {
  protocolVersion,
  type ExtensionSubscriptionMessage,
} from "./protocol";
import type { PageWindow } from "./transport";

const idBlockSize = 2 ** 20;
const subscriptionIds = new WeakMap<PageWindow, number>();

type SubscriptionCallbacks = {
  onSnapshot: (snapshot: unknown) => void;
  onError?: (error: EscpostError) => void;
  isSnapshot: (snapshot: unknown) => boolean;
};

export class SubscriptionTransport {
  private readonly callbacks = new Map<number, SubscriptionCallbacks>();
  private listening = false;
  private readonly initialSubscriptionId = Math.floor(Math.random() * 2 ** 32) * idBlockSize;

  constructor(private readonly page?: PageWindow) {}

  subscribe<T>(
    onSnapshot: (snapshot: T) => void,
    options: { onError?: (error: EscpostError) => void } = {},
    isSnapshot: (snapshot: unknown) => snapshot is T = (_snapshot: unknown): _snapshot is T => true,
  ): () => void {
    const page = this.pageWindow();
    this.ensureListening(page);
    const subscriptionId = allocateSubscriptionId(page, this.initialSubscriptionId);
    this.callbacks.set(subscriptionId, {
      onSnapshot: onSnapshot as (snapshot: unknown) => void,
      onError: options.onError,
      isSnapshot,
    });
    page.postMessage(
      {
        source: "escpost-page",
        kind: "subscribe",
        subscriptionId,
        op: "printers.events",
        protocol: protocolVersion,
      },
      "*",
    );

    let cancelled = false;
    return () => {
      if (cancelled) return;
      cancelled = true;
      this.callbacks.delete(subscriptionId);
      this.pageWindow().postMessage(
        { source: "escpost-page", kind: "unsubscribe", subscriptionId },
        "*",
      );
    };
  }

  private ensureListening(page: PageWindow): void {
    if (this.listening) return;
    this.listening = true;
    page.addEventListener("message", (event) => this.receive(event));
  }

  private receive(event: MessageEvent): void {
    if (event.source !== this.pageWindow()) return;
    const message = event.data;
    if (!isExtensionSubscriptionMessage(message)) return;

    const callbacks = this.callbacks.get(message.subscriptionId);
    if (callbacks === undefined) return;

    if (message.kind === "failure") {
      callbacks.onError?.(fromSerializedError(message.error));
      return;
    }
    if (!callbacks.isSnapshot(message.data)) {
      callbacks.onError?.(
        new EscpostError("PROTOCOL_MISMATCH", "The extension returned an invalid printer inventory snapshot."),
      );
      return;
    }
    callbacks.onSnapshot(message.data);
  }

  private pageWindow(): PageWindow {
    const page = this.page ?? (globalThis.window as unknown as PageWindow | undefined);
    if (page === undefined) {
      throw new EscpostError("EXTENSION_UNAVAILABLE", "The browser page relay is unavailable.");
    }
    return page;
  }
}

function allocateSubscriptionId(page: PageWindow, initialSubscriptionId: number): number {
  const subscriptionId = (subscriptionIds.get(page) ?? initialSubscriptionId) + 1;
  subscriptionIds.set(page, subscriptionId);
  return subscriptionId;
}

function isExtensionSubscriptionMessage(value: unknown): value is ExtensionSubscriptionMessage {
  if (typeof value !== "object" || value === null) return false;
  const message = value as {
    source?: unknown;
    subscriptionId?: unknown;
    kind?: unknown;
  };
  return (
    message.source === "escpost-extension" &&
    typeof message.subscriptionId === "number" &&
    (message.kind === "snapshot" || message.kind === "failure")
  );
}
