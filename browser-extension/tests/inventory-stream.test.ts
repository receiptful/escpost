import { afterEach, expect, test, vi } from "vitest";
import { installInventoryStreams } from "../src/inventory-stream";
import type { InventoryStreamCallbacks, WirePrinterInventory } from "../src/daemon";

type PortMessage =
  | { kind: "subscribe"; subscriptionId: number; protocol: number }
  | { kind: "unsubscribe"; subscriptionId: number };

class ControlledPort {
  readonly posted: unknown[] = [];
  readonly disconnect = vi.fn();
  readonly sender: { url?: string } | undefined;
  private messageListener: ((message: unknown) => void) | undefined;
  private disconnectListener: (() => void) | undefined;

  constructor(
    readonly name = "escpost-printers",
    sender: { url?: string } | null = { url: "https://shop.example/orders/7" },
  ) {
    this.sender = sender ?? undefined;
  }

  readonly onMessage = {
    addListener: (listener: (message: unknown) => void) => { this.messageListener = listener; },
  };
  readonly onDisconnect = {
    addListener: (listener: () => void) => { this.disconnectListener = listener; },
  };

  postMessage(message: unknown): void { this.posted.push(message); }
  receive(message: PortMessage | unknown): void { this.messageListener?.(message); }
  drop(): void { this.disconnectListener?.(); }
}

class ControlledDaemon {
  readonly attempts: Array<{
    callbacks: InventoryStreamCallbacks;
    signal: AbortSignal;
    resolve: () => void;
  }> = [];

  openInventoryStream(callbacks: InventoryStreamCallbacks, signal: AbortSignal): Promise<void> {
    return new Promise<void>((resolve) => {
      const attempt = { callbacks, signal, resolve };
      this.attempts.push(attempt);
      signal.addEventListener("abort", () => resolve(), { once: true });
    });
  }

  emit(snapshot: WirePrinterInventory, index = this.attempts.length - 1): void {
    this.attempts[index]?.callbacks.onSnapshot(snapshot);
  }

  end(index = this.attempts.length - 1): void {
    const attempt = this.attempts[index];
    if (attempt === undefined) throw new Error("Expected an open daemon stream.");
    attempt.callbacks.onError(new Error("connection lost"));
    attempt.resolve();
  }
}

const snapshot: WirePrinterInventory = {
  updated_at: "2026-09-01T12:00:00Z",
  warning: null,
  printers: [{
    name: "counter",
    transport: "network",
    availability: "connected",
    profile: null,
    connection: { type: "network", host: "192.0.2.7", port: 9100 },
  }],
};

function setup(granted = true) {
  let connect: ((port: ControlledPort) => void) | undefined;
  const runtime = { onConnect: { addListener: vi.fn((listener) => { connect = listener; }) } };
  const grants = {
    contains: vi.fn(async () => granted),
    onRemoved: vi.fn(),
  };
  const daemon = new ControlledDaemon();
  installInventoryStreams(runtime, { grants, daemon });
  return { connect: (port: ControlledPort) => connect?.(port), grants, daemon };
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

afterEach(() => {
  vi.useRealTimers();
});

test("shares one authorized daemon stream across every subscription on a port", async () => {
  // Break caught: opening one SSE fetch per id defeats document-level
  // multiplexing and makes one unsubscribe abort another id's ownership.
  const { connect, grants, daemon } = setup();
  const port = new ControlledPort();
  connect(port);
  port.receive({ kind: "subscribe", subscriptionId: 17, protocol: 1 });
  port.receive({ kind: "subscribe", subscriptionId: 18, protocol: 1 });
  await flush();

  expect(grants.contains).toHaveBeenCalledOnce();
  expect(grants.contains).toHaveBeenCalledWith("https://shop.example/*");
  expect(daemon.attempts).toHaveLength(1);

  daemon.emit(snapshot);
  expect(port.posted).toEqual([
    { kind: "snapshot", subscriptionId: 17, data: snapshot },
    { kind: "snapshot", subscriptionId: 18, data: snapshot },
  ]);

  port.receive({ kind: "unsubscribe", subscriptionId: 17 });
  expect(daemon.attempts[0]?.signal.aborted).toBe(false);
  port.receive({ kind: "unsubscribe", subscriptionId: 18 });
  expect(daemon.attempts[0]?.signal.aborted).toBe(true);
});

test("aborts immediately on port disconnect and suppresses late daemon callbacks", async () => {
  // Break caught: retaining a fetch or callback after its document port dies
  // leaks daemon traffic and can deliver an event into a replacement document.
  const { connect, daemon } = setup();
  const port = new ControlledPort();
  connect(port);
  port.receive({ kind: "subscribe", subscriptionId: 17, protocol: 1 });
  await flush();

  port.drop();
  expect(daemon.attempts[0]?.signal.aborted).toBe(true);
  daemon.emit(snapshot);
  daemon.attempts[0]?.callbacks.onError(new Error("late failure"));
  await flush();
  expect(port.posted).toEqual([]);
});

test("reports each disconnect once and retries at 150, 300, 600, then bounded 1000 ms delays", async () => {
  // Break caught: immediate/unbounded retry spins the MV3 worker and floods the
  // loopback daemon, while dropping ids prevents recovery after disconnection.
  vi.useFakeTimers();
  const { connect, daemon } = setup();
  const port = new ControlledPort();
  connect(port);
  port.receive({ kind: "subscribe", subscriptionId: 17, protocol: 1 });
  await flush();

  const delays = [150, 300, 600, 1_000, 1_000];
  for (const [index, delay] of delays.entries()) {
    daemon.end(index);
    await flush();
    expect(port.posted.at(-1)).toEqual({
      kind: "failure",
      subscriptionId: 17,
      error: {
        code: "DAEMON_UNAVAILABLE",
        message: "The local ESCPost daemon inventory stream disconnected.",
      },
    });
    expect(port.posted).toHaveLength(index + 1);
    vi.advanceTimersByTime(delay - 1);
    await flush();
    expect(daemon.attempts).toHaveLength(index + 1);
    vi.advanceTimersByTime(1);
    await flush();
    expect(daemon.attempts).toHaveLength(index + 2);
  }
});

test("cancels a pending reconnect timer on final unsubscribe", async () => {
  // Break caught: a retry timer outliving the final id opens daemon traffic
  // after the page has explicitly released its last subscription.
  vi.useFakeTimers();
  const { connect, daemon } = setup();
  const port = new ControlledPort();
  connect(port);
  port.receive({ kind: "subscribe", subscriptionId: 17, protocol: 1 });
  await flush();

  daemon.end();
  await flush();
  port.receive({ kind: "unsubscribe", subscriptionId: 17 });
  vi.advanceTimersByTime(2_000);
  await flush();
  expect(daemon.attempts).toHaveLength(1);
});

test("rechecks the explicit page grant before a reconnect opens daemon traffic", async () => {
  // Break caught: caching a granted port forever lets a revoked page start a
  // replacement SSE fetch after its original daemon transport disconnects.
  vi.useFakeTimers();
  let connect: ((port: ControlledPort) => void) | undefined;
  const runtime = { onConnect: { addListener: vi.fn((listener) => { connect = listener; }) } };
  const grants = {
    contains: vi.fn()
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(false),
    onRemoved: vi.fn(),
  };
  const daemon = new ControlledDaemon();
  installInventoryStreams(runtime, { grants, daemon });
  const port = new ControlledPort();
  connect?.(port);
  port.receive({ kind: "subscribe", subscriptionId: 17, protocol: 1 });
  await flush();

  daemon.end();
  await flush();
  vi.advanceTimersByTime(150);
  await flush();

  expect(grants.contains).toHaveBeenCalledTimes(2);
  expect(daemon.attempts).toHaveLength(1);
  expect(port.posted.at(-1)).toEqual({
    kind: "failure",
    subscriptionId: 17,
    error: { code: "ORIGIN_NOT_GRANTED", message: "This page origin is not granted access to ESCPost." },
  });
});

test("aborts and removes a live stream as soon as its site permission is revoked", async () => {
  // Break caught: checking permission only when opening or reconnecting leaves
  // an already-open SSE stream delivering inventory after its site's grant is
  // removed from Chrome.
  let connect: ((port: ControlledPort) => void) | undefined;
  let removeGrant: ((patterns: string[]) => void) | undefined;
  const runtime = { onConnect: { addListener: vi.fn((listener) => { connect = listener; }) } };
  const grants = {
    contains: vi.fn(async () => true),
    onRemoved: vi.fn((listener: (patterns: string[]) => void) => {
      removeGrant = listener;
    }),
  };
  const daemon = new ControlledDaemon();
  installInventoryStreams(runtime, { grants, daemon });
  const port = new ControlledPort();
  connect?.(port);
  port.receive({ kind: "subscribe", subscriptionId: 17, protocol: 1 });
  await flush();

  daemon.emit(snapshot);
  removeGrant?.(["https://shop.example/*"]);

  expect(daemon.attempts[0]?.signal.aborted).toBe(true);
  daemon.emit({ ...snapshot, updated_at: "2026-09-01T12:01:00Z" });
  port.receive({ kind: "subscribe", subscriptionId: 18, protocol: 1 });
  await flush();
  expect(daemon.attempts).toHaveLength(1);
  expect(port.posted).toEqual([
    { kind: "snapshot", subscriptionId: 17, data: snapshot },
    {
      kind: "failure",
      subscriptionId: 17,
      error: {
        code: "ORIGIN_NOT_GRANTED",
        message: "This page origin is not granted access to ESCPost.",
      },
    },
  ]);
});

test("rejects wrong ports and untrusted sender URLs before grant checks or daemon traffic", async () => {
  // Break caught: trusting a page-supplied origin or the fixed loopback host
  // lets non-web/ungranted documents turn extension host access into SSE access.
  const { connect, grants, daemon } = setup();
  const ports = [
    new ControlledPort("other-port"),
    new ControlledPort("escpost-printers", null),
    new ControlledPort("escpost-printers", {}),
    new ControlledPort("escpost-printers", { url: "null" }),
    new ControlledPort("escpost-printers", { url: "chrome-extension://extension-id/page.html" }),
    new ControlledPort("escpost-printers", { url: "http://127.0.0.1:9000/orders" }),
  ];

  for (const port of ports) {
    connect(port);
    port.receive({ kind: "subscribe", subscriptionId: 17, protocol: 1 });
  }
  await flush();

  expect(grants.contains).not.toHaveBeenCalled();
  expect(daemon.attempts).toEqual([]);
});

test("fails a denied port subscription without opening a daemon stream", async () => {
  // Break caught: beginning the SSE fetch before the asynchronous explicit
  // host-grant decision exposes daemon inventory to a denied document.
  const { connect, grants, daemon } = setup(false);
  const port = new ControlledPort();
  connect(port);
  port.receive({ kind: "subscribe", subscriptionId: 17, protocol: 1 });
  await flush();

  expect(grants.contains).toHaveBeenCalledWith("https://shop.example/*");
  expect(daemon.attempts).toEqual([]);
  expect(port.posted).toEqual([{
    kind: "failure",
    subscriptionId: 17,
    error: { code: "ORIGIN_NOT_GRANTED", message: "This page origin is not granted access to ESCPost." },
  }]);
});
