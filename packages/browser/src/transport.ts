import { EscpostError, fromSerializedError } from "./errors";
import {
  protocolVersion,
  type ExtensionReply,
  type PageMessage,
  type PageOperation,
} from "./protocol";

const idBlockSize = 2 ** 20;

export type PageWindow = {
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  postMessage(message: PageMessage, targetOrigin?: string): void;
};

type PendingRequest = {
  resolve: (data: unknown) => void;
  reject: (error: EscpostError) => void;
  timeout: ReturnType<typeof setTimeout>;
};

export class PageTransport {
  private readonly pending = new Map<number, PendingRequest>();
  private listening = false;
  private nextId = Math.floor(Math.random() * 2 ** 32) * idBlockSize;

  constructor(private readonly page?: PageWindow) {}

  request<T>(op: PageOperation, payload: unknown, timeoutMs: number): Promise<T> {
    this.ensureListening();
    const id = ++this.nextId;

    return new Promise<T>((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        reject(new EscpostError("EXTENSION_UNAVAILABLE", `The extension did not answer ${op}.`));
      }, timeoutMs);

      this.pending.set(id, { resolve: resolve as (data: unknown) => void, reject, timeout });
      this.pageWindow().postMessage(
        { source: "escpost-page", protocol: protocolVersion, id, op, payload },
        "*",
      );
    });
  }

  private ensureListening(): void {
    if (this.listening) return;
    this.listening = true;
    this.pageWindow().addEventListener("message", (event) => this.receive(event));
  }

  private receive(event: MessageEvent): void {
    if (event.source !== this.pageWindow()) return;
    const reply = event.data;
    if (!isExtensionReply(reply)) return;

    const pending = this.pending.get(reply.id);
    if (!pending) return;

    clearTimeout(pending.timeout);
    this.pending.delete(reply.id);
    if (reply.ok === false) {
      pending.reject(fromSerializedError(reply.error));
      return;
    }
    pending.resolve(reply.data);
  }

  private pageWindow(): PageWindow {
    const page = this.page ?? (globalThis.window as unknown as PageWindow | undefined);
    if (page === undefined) {
      throw new EscpostError("EXTENSION_UNAVAILABLE", "The browser page relay is unavailable.");
    }
    return page;
  }
}

function isExtensionReply(value: unknown): value is ExtensionReply {
  if (typeof value !== "object" || value === null) return false;
  const reply = value as { source?: unknown; id?: unknown; ok?: unknown };
  return (
    reply.source === "escpost-extension" &&
    typeof reply.id === "number" &&
    (reply.ok === true || reply.ok === false)
  );
}
