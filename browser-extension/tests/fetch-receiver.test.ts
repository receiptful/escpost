import { expect, test } from "vitest";
import { DaemonClient } from "../src/daemon";
import { DaemonPortStore } from "../src/daemon-port";

class MemoryStorageArea {
  readonly values: Record<string, unknown> = { daemonBaseUrl: "http://127.0.0.1:9000" };
  async get(key: string): Promise<Record<string, unknown>> { return { [key]: this.values[key] }; }
  async set(values: Record<string, unknown>): Promise<void> { Object.assign(this.values, values); }
  async remove(key: string): Promise<void> { delete this.values[key]; }
}

class PausedInvalidationStorageArea extends MemoryStorageArea {
  private releaseRemove: () => void = () => undefined;
  private readonly removeMayContinue = new Promise<void>((resolve) => { this.releaseRemove = resolve; });
  private resolveRemoveStarted: () => void = () => undefined;
  readonly removeStarted = new Promise<void>((resolve) => { this.resolveRemoveStarted = resolve; });

  override async remove(key: string): Promise<void> {
    this.resolveRemoveStarted();
    await this.removeMayContinue;
    await super.remove(key);
  }

  continueInvalidation(): void {
    this.releaseRemove();
  }
}

const inventory = {
  updated_at: "2026-09-01T10:30:00Z", warning: null,
  printers: [{
    name: "counter", transport: "network", availability: "connected", profile: null,
    connection: { type: "network", host: "192.0.2.7", port: 9100 },
  }],
};

function stream(parts: string[]): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  return new ReadableStream({
    start(controller) {
      for (const part of parts) controller.enqueue(encoder.encode(part));
      controller.close();
    },
  });
}

test("keeps the native fetch receiver when using the default fetch", async () => {
  // Break caught: extracting fetch without binding its receiver makes Chrome's
  // native implementation throw before the daemon can be contacted.
  const originalFetch = globalThis.fetch;
  let receiver: unknown;
  globalThis.fetch = function (this: unknown): Promise<Response> {
    receiver = this;
    return Promise.resolve(new Response(JSON.stringify({ ok: true })));
  } as typeof fetch;
  try {
    const daemon = new DaemonClient(new DaemonPortStore(new MemoryStorageArea()));
    await expect(daemon.health()).resolves.toBe(true);
    expect(receiver).toBe(globalThis);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("parses complete default-message inventory snapshots split across SSE chunks", async () => {
  // Break caught: accepting named/non-snapshot events or incomplete chunks
  // loses live inventory updates when an HTTP stream splits a message.
  const snapshots: unknown[] = [];
  const errors: Error[] = [];
  const daemon = new DaemonClient(
    new DaemonPortStore(new MemoryStorageArea()),
    async () => new Response(stream([`data: ${JSON.stringify(inventory).slice(0, 38)}`, `${JSON.stringify(inventory).slice(38)}\n\n`])),
  );
  const controller = new AbortController();

  await daemon.openInventoryStream({ onSnapshot: (snapshot) => snapshots.push(snapshot), onError: (error) => errors.push(error) }, controller.signal);
  expect(snapshots).toEqual([inventory]);
  expect(errors).toEqual([]);
});

test("waits for the blank SSE delimiter before joining multi-line data", async () => {
  // Break caught: treating a chunk-ending newline as an event boundary emits
  // incomplete JSON whenever an SSE event spans transport chunks.
  const snapshots: unknown[] = [];
  const errors: Error[] = [];
  const encoded = JSON.stringify(inventory);
  const splitAt = encoded.indexOf(",") + 1;
  const daemon = new DaemonClient(
    new DaemonPortStore(new MemoryStorageArea()),
    async () => new Response(stream([`data: ${encoded.slice(0, splitAt)}\n`, `data: ${encoded.slice(splitAt)}\n\n`])),
  );

  await daemon.openInventoryStream({ onSnapshot: (snapshot) => snapshots.push(snapshot), onError: (error) => errors.push(error) }, new AbortController().signal);
  expect(snapshots).toEqual([inventory]);
  expect(errors).toEqual([]);
});

test("parses empty default events, comments, named events, and CRLF split across chunks", async () => {
  // Break caught: trimming the event field changes an empty default event or
  // treats a named SSE event as inventory, while CRLF chunking loses framing.
  const snapshots: unknown[] = [];
  const errors: Error[] = [];
  const ignored = JSON.stringify(inventory);
  const daemon = new DaemonClient(
    new DaemonPortStore(new MemoryStorageArea()),
    async () => new Response(stream([
      ": daemon keepalive\r",
      `\nevent:\r\ndata: ${JSON.stringify(inventory)}\r\n\r\nevent: status\r\ndata: ${ignored}\r\n\r\n`,
    ])),
  );

  await daemon.openInventoryStream({ onSnapshot: (snapshot) => snapshots.push(snapshot), onError: (error) => errors.push(error) }, new AbortController().signal);
  expect(snapshots).toEqual([inventory]);
  expect(errors).toEqual([]);
});

test("does not fetch when an inventory stream is already aborted", async () => {
  // Break caught: a pre-aborted caller still opening an SSE socket leaks a
  // daemon connection after the UI has already disposed its subscription.
  let calls = 0;
  const daemon = new DaemonClient(new DaemonPortStore(new MemoryStorageArea()), async () => {
    calls += 1;
    return new Response();
  });
  const controller = new AbortController();
  controller.abort();

  await daemon.openInventoryStream({ onSnapshot: () => undefined, onError: () => undefined }, controller.signal);
  expect(calls).toBe(0);
});

test("passes the inventory signal to a pending fetch and stops without rediscovery on abort", async () => {
  // Break caught: an aborted stream-open fetch is treated as a transport
  // failure, causing discovery and another connection after cancellation.
  let calls = 0;
  let receivedSignal: AbortSignal | undefined;
  const controller = new AbortController();
  let rejectPending: (error: Error) => void = () => undefined;
  let fetchStarted: () => void;
  const started = new Promise<void>((resolve) => { fetchStarted = resolve; });
  const daemon = new DaemonClient(new DaemonPortStore(new MemoryStorageArea()), async (_input, init) => {
    calls += 1;
    receivedSignal = init?.signal ?? undefined;
    if (calls > 1) return new Response(null, { status: 503 });
    fetchStarted();
    return await new Promise<Response>((_resolve, reject) => {
      rejectPending = reject;
      init?.signal?.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError")), { once: true });
    });
  });
  const errors: Error[] = [];

  const opened = daemon.openInventoryStream({ onSnapshot: () => undefined, onError: (error) => errors.push(error) }, controller.signal);
  await started;
  controller.abort();
  rejectPending(new DOMException("Aborted", "AbortError"));
  await opened;

  expect(receivedSignal).toBe(controller.signal);
  expect(calls).toBe(1);
  expect(errors).toEqual([]);
});

test("does not rediscover after aborting during a pending stream cache invalidation", async () => {
  // Break caught: cancellation during storage cleanup reaches the retry path
  // and opens discovery requests after the stream owner has gone away.
  const storage = new PausedInvalidationStorageArea();
  const ports = new DaemonPortStore(storage);
  const calls: string[] = [];
  const errors: Error[] = [];
  const controller = new AbortController();
  const daemon = new DaemonClient(ports, async (input) => {
    calls.push(String(input));
    throw new TypeError("connection reset");
  });

  const opened = daemon.openInventoryStream({ onSnapshot: () => undefined, onError: (error) => errors.push(error) }, controller.signal);
  await storage.removeStarted;
  controller.abort();
  storage.continueInvalidation();
  await opened;

  expect(calls).toEqual(["http://127.0.0.1:9000/api/printers/list/events"]);
  expect(errors).toEqual([]);
});

test("does not report a null stream body when abort wins as fetch resolves", async () => {
  // Break caught: the null-body error callback fires after stream cancellation
  // even though no reader should be opened for an aborted subscription.
  const controller = new AbortController();
  const errors: Error[] = [];
  const daemon = new DaemonClient(new DaemonPortStore(new MemoryStorageArea()), async () => {
    controller.abort();
    return new Response();
  });

  await daemon.openInventoryStream({ onSnapshot: () => undefined, onError: (error) => errors.push(error) }, controller.signal);
  expect(errors).toEqual([]);
});

test("does not emit a chunk that resolves after the stream is aborted", async () => {
  // Break caught: omitting the abort gate immediately after reader.read()
  // forwards a queued snapshot after the owning subscription is cancelled.
  const controller = new AbortController();
  let read = false;
  const body = {
    getReader() {
      return {
        async read() {
          if (read) return { done: true, value: undefined };
          read = true;
          controller.abort();
          return {
            done: false,
            value: new TextEncoder().encode(`data: ${JSON.stringify(inventory)}\n\n`),
          };
        },
        async cancel() {},
        releaseLock() {},
      };
    },
  };
  const snapshots: unknown[] = [];
  const errors: Error[] = [];
  const daemon = new DaemonClient(
    new DaemonPortStore(new MemoryStorageArea()),
    async () => ({ ok: true, body }) as unknown as Response,
  );

  await daemon.openInventoryStream({
    onSnapshot: (snapshot) => snapshots.push(snapshot),
    onError: (error) => errors.push(error),
  }, controller.signal);

  expect(snapshots).toEqual([]);
  expect(errors).toEqual([]);
});

test("absorbs a rejected reader cancellation without emitting callbacks", async () => {
  // Break caught: discarding reader.cancel() from the abort listener leaks its
  // rejected promise as an unhandled rejection after unsubscribe/disconnect.
  let resolveRead: (result: ReadableStreamReadResult<Uint8Array>) => void = () => undefined;
  let observedRead: () => void = () => undefined;
  const readStarted = new Promise<void>((resolve) => { observedRead = resolve; });
  const pendingRead = new Promise<ReadableStreamReadResult<Uint8Array>>((resolve) => { resolveRead = resolve; });
  const cancellationFailure = new Error("reader cancellation failed");
  const body = {
    getReader() {
      return {
        read() {
          observedRead();
          return pendingRead;
        },
        async cancel() { throw cancellationFailure; },
        releaseLock() {},
      };
    },
  };
  const snapshots: unknown[] = [];
  const errors: Error[] = [];
  const daemon = new DaemonClient(
    new DaemonPortStore(new MemoryStorageArea()),
    async () => ({ ok: true, body }) as unknown as Response,
  );
  const controller = new AbortController();
  const opened = daemon.openInventoryStream({
    onSnapshot: (snapshot) => snapshots.push(snapshot),
    onError: (error) => errors.push(error),
  }, controller.signal);
  await readStarted;

  controller.abort();
  resolveRead({
    done: false,
    value: new TextEncoder().encode(`data: ${JSON.stringify(inventory)}\n\n`),
  });
  await opened;

  expect(snapshots).toEqual([]);
  expect(errors).toEqual([]);
});

test("stops buffered SSE callbacks immediately when a snapshot aborts the stream", async () => {
  // Break caught: parsing every complete event in a buffered chunk after the
  // first callback cancels emits a later snapshot or failure to a dead owner.
  const controller = new AbortController();
  const body = stream([
    `data: ${JSON.stringify(inventory)}\n\ndata: {"printers":[]}\n\n`,
  ]);
  const snapshots: unknown[] = [];
  const errors: Error[] = [];
  const daemon = new DaemonClient(
    new DaemonPortStore(new MemoryStorageArea()),
    async () => new Response(body),
  );

  await daemon.openInventoryStream({
    onSnapshot: (snapshot) => {
      snapshots.push(snapshot);
      controller.abort();
    },
    onError: (error) => errors.push(error),
  }, controller.signal);

  expect(snapshots).toEqual([inventory]);
  expect(errors).toEqual([]);
});

test("reports invalid SSE snapshots and cancels the reader when aborted", async () => {
  // Break caught: forwarding malformed stream data or leaving a reader open
  // after callers cancel leaks a live daemon connection into later views.
  let cancelled = false;
  const body = new ReadableStream<Uint8Array>({
    start(controller) { controller.enqueue(new TextEncoder().encode("data: {\"printers\":[]}\n\n")); },
    cancel() { cancelled = true; },
  });
  const errors: Error[] = [];
  const daemon = new DaemonClient(new DaemonPortStore(new MemoryStorageArea()), async () => new Response(body));
  const controller = new AbortController();
  let observedInvalidSnapshot: () => void;
  const invalidSnapshot = new Promise<void>((resolve) => { observedInvalidSnapshot = resolve; });
  const opened = daemon.openInventoryStream({
    onSnapshot: () => undefined,
    onError: (error) => { errors.push(error); observedInvalidSnapshot(); },
  }, controller.signal);
  await invalidSnapshot;
  controller.abort();

  await opened;
  expect(errors).toHaveLength(1);
  expect(errors[0]?.message).toBe("The daemon sent an invalid printer inventory.");
  expect(cancelled).toBe(true);
});
