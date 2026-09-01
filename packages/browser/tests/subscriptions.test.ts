import { afterEach, expect, test, vi } from "vitest";
import { escpost } from "../src/index";
import type { PageMessage } from "../src/protocol";
import { SubscriptionTransport } from "../src/subscriptions";
import type { PageWindow } from "../src/transport";
import type { PrinterInventory } from "../src/types";

class FakePageWindow implements PageWindow {
  readonly posted: unknown[] = [];
  private readonly listeners: Array<(event: MessageEvent) => void> = [];

  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void {
    this.listeners.push(listener);
  }

  postMessage(message: PageMessage): void {
    this.posted.push(message);
  }

  dispatchExtensionMessage(message: unknown): void {
    for (const listener of this.listeners) {
      listener({ data: message, source: this } as unknown as MessageEvent);
    }
  }
}

const wireSnapshot = {
  updated_at: "2026-09-01T11:00:00Z",
  warning: null,
  printers: [
    {
      name: "counter",
      transport: "network",
      availability: "connected",
      profile: "80mm",
      connection: { type: "network", host: "192.0.2.10", port: 9100 },
    },
  ],
};

const mappedSnapshot: PrinterInventory = {
  updatedAt: "2026-09-01T11:00:00Z",
  warning: null,
  printers: [
    {
      name: "counter",
      transport: "network",
      availability: "connected",
      profile: "80mm",
      connection: { type: "network", host: "192.0.2.10", port: 9100 },
    },
  ],
};

let pageRelay: FakePageWindow | undefined;

afterEach(() => vi.restoreAllMocks());

function installPageRelay(): FakePageWindow {
  pageRelay ??= new FakePageWindow();
  pageRelay.posted.length = 0;
  Object.assign(globalThis, { window: pageRelay });
  return pageRelay;
}

function subscriptionId(page: FakePageWindow, index = 0): number {
  const message = page.posted.filter(
    (candidate): candidate is { kind: "subscribe"; subscriptionId: number } =>
      typeof candidate === "object" &&
      candidate !== null &&
      (candidate as { kind?: unknown }).kind === "subscribe" &&
      typeof (candidate as { subscriptionId?: unknown }).subscriptionId === "number",
  )[index];
  if (message === undefined) throw new Error("Expected a subscription request.");
  return message.subscriptionId;
}

test("subscribes for mapped snapshots without falling back to a list request", () => {
  // Break caught: synthesizing a first snapshot with list() makes subscription
  // startup block on probes instead of waiting for the SSE-backed relay event.
  const page = installPageRelay();
  const snapshots: PrinterInventory[] = [];
  const stop = escpost.printers.subscribe((snapshot) => snapshots.push(snapshot));
  const id = subscriptionId(page);

  expect(page.posted).toEqual([
    { source: "escpost-page", kind: "subscribe", subscriptionId: id, op: "printers.events", protocol: 1 },
  ]);

  page.dispatchExtensionMessage({
    source: "escpost-extension",
    subscriptionId: id,
    kind: "snapshot",
    data: wireSnapshot,
  });

  expect(snapshots).toEqual([mappedSnapshot]);
  stop();
  expect(page.posted.at(-1)).toEqual({
    source: "escpost-page",
    kind: "unsubscribe",
    subscriptionId: id,
  });
});

test("delivers every later stream snapshot", () => {
  // Break caught: retaining just the initial snapshot leaves subscribers with
  // stale printer availability after the daemon emits a replacement snapshot.
  const page = installPageRelay();
  const snapshots: PrinterInventory[] = [];
  const stop = escpost.printers.subscribe((snapshot) => snapshots.push(snapshot));
  const id = subscriptionId(page);
  const laterWireSnapshot = { ...wireSnapshot, updated_at: "2026-09-01T11:05:00Z", warning: "Probe slow." };

  page.dispatchExtensionMessage({ source: "escpost-extension", subscriptionId: id, kind: "snapshot", data: wireSnapshot });
  page.dispatchExtensionMessage({
    source: "escpost-extension",
    subscriptionId: id,
    kind: "snapshot",
    data: laterWireSnapshot,
  });

  expect(snapshots).toEqual([
    mappedSnapshot,
    { ...mappedSnapshot, updatedAt: "2026-09-01T11:05:00Z", warning: "Probe slow." },
  ]);
  stop();
});

test("reports a stream failure without replacing the last good snapshot", () => {
  // Break caught: treating a relay failure as an empty inventory erases a
  // caller's last known printer state and hides the typed failure reason.
  const page = installPageRelay();
  const snapshots: PrinterInventory[] = [];
  const errors: unknown[] = [];
  const stop = escpost.printers.subscribe(
    (snapshot) => snapshots.push(snapshot),
    { onError: (error) => errors.push(error) },
  );
  const id = subscriptionId(page);

  page.dispatchExtensionMessage({
    source: "escpost-extension",
    subscriptionId: id,
    kind: "failure",
    error: { code: "DAEMON_UNAVAILABLE", message: "The daemon disconnected." },
  });

  expect(snapshots).toEqual([]);
  expect(errors).toMatchObject([{ name: "EscpostError", code: "DAEMON_UNAVAILABLE", message: "The daemon disconnected." }]);
  stop();
});

test("rejects malformed snapshot envelopes without replacing a valid inventory", () => {
  // Break caught: forwarding a missing snapshot payload into the inventory
  // mapper throws a native exception instead of reporting a typed protocol error.
  const page = installPageRelay();
  const snapshots: PrinterInventory[] = [];
  const errors: unknown[] = [];
  const stop = escpost.printers.subscribe(
    (snapshot) => snapshots.push(snapshot),
    { onError: (error) => errors.push(error) },
  );
  const id = subscriptionId(page);

  page.dispatchExtensionMessage({ source: "escpost-extension", subscriptionId: id, kind: "snapshot", data: wireSnapshot });
  expect(() => {
    page.dispatchExtensionMessage({ source: "escpost-extension", subscriptionId: id, kind: "snapshot" });
  }).not.toThrow();

  expect(snapshots).toEqual([mappedSnapshot]);
  expect(errors).toMatchObject([
    { name: "EscpostError", code: "PROTOCOL_MISMATCH" },
  ]);
  stop();
});

test("rejects malformed nested snapshot data and delivers a later valid snapshot", () => {
  // Break caught: accepting a partial wire printer publishes invalid public
  // inventory facts instead of retaining the caller's last valid snapshot.
  const page = installPageRelay();
  const snapshots: PrinterInventory[] = [];
  const errors: unknown[] = [];
  const stop = escpost.printers.subscribe(
    (snapshot) => snapshots.push(snapshot),
    { onError: (error) => errors.push(error) },
  );
  const id = subscriptionId(page);
  const laterWireSnapshot = { ...wireSnapshot, updated_at: "2026-09-01T11:10:00Z" };

  page.dispatchExtensionMessage({ source: "escpost-extension", subscriptionId: id, kind: "snapshot", data: wireSnapshot });
  expect(() => {
    page.dispatchExtensionMessage({
      source: "escpost-extension",
      subscriptionId: id,
      kind: "snapshot",
      data: {
        ...wireSnapshot,
        printers: [
          {
            ...wireSnapshot.printers[0],
            connection: { type: "network", host: 192, port: 9100 },
          },
        ],
      },
    });
  }).not.toThrow();
  page.dispatchExtensionMessage({
    source: "escpost-extension",
    subscriptionId: id,
    kind: "snapshot",
    data: laterWireSnapshot,
  });

  expect(snapshots).toEqual([mappedSnapshot, { ...mappedSnapshot, updatedAt: "2026-09-01T11:10:00Z" }]);
  expect(errors).toMatchObject([
    { name: "EscpostError", code: "PROTOCOL_MISMATCH" },
  ]);
  stop();
});

test("cancels a subscription once and ignores snapshots after cancellation", () => {
  // Break caught: leaving callbacks registered or posting duplicate cancels
  // after a repeated stop leaks subscriptions and delivers stale events.
  const page = installPageRelay();
  const snapshots: PrinterInventory[] = [];
  const stop = escpost.printers.subscribe((snapshot) => snapshots.push(snapshot));
  const id = subscriptionId(page);

  stop();
  stop();
  page.dispatchExtensionMessage({
    source: "escpost-extension",
    subscriptionId: id,
    kind: "snapshot",
    data: wireSnapshot,
  });

  expect(snapshots).toEqual([]);
  expect(page.posted.filter((message) => (message as { kind?: unknown }).kind === "unsubscribe")).toEqual([
    { source: "escpost-page", kind: "unsubscribe", subscriptionId: id },
  ]);
});

test("keeps matching stream messages isolated between subscription transport instances", () => {
  // Break caught: independently random subscription-ID blocks can collide, so
  // two SDK instances both consume one matching extension stream message.
  vi.spyOn(Math, "random").mockReturnValue(0);
  const page = new FakePageWindow();
  const first = new SubscriptionTransport(page);
  const second = new SubscriptionTransport(page);
  const firstSnapshots: unknown[] = [];
  const secondSnapshots: unknown[] = [];
  const stopFirst = first.subscribe((snapshot) => firstSnapshots.push(snapshot));
  const firstId = subscriptionId(page);
  const stopSecond = second.subscribe((snapshot) => secondSnapshots.push(snapshot));
  const secondId = subscriptionId(page, 1);

  expect(firstId).toBe(1);
  expect(secondId).toBe(2);
  page.dispatchExtensionMessage({
    source: "escpost-extension",
    subscriptionId: firstId,
    kind: "snapshot",
    data: wireSnapshot,
  });

  expect(firstSnapshots).toEqual([wireSnapshot]);
  expect(secondSnapshots).toEqual([]);
  stopFirst();
  stopSecond();
});
