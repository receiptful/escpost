import { expect, test } from "vitest";
import { DaemonClient } from "../src/daemon";
import { DaemonPortStore } from "../src/daemon-port";

class MemoryStorageArea {
  readonly values: Record<string, unknown> = { daemonBaseUrl: "http://127.0.0.1:9000" };
  async get(key: string): Promise<Record<string, unknown>> { return { [key]: this.values[key] }; }
  async set(values: Record<string, unknown>): Promise<void> { Object.assign(this.values, values); }
  async remove(key: string): Promise<void> { delete this.values[key]; }
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
