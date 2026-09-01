import { expect, test, vi } from "vitest";
import { installRelay } from "../src/relay";

type WindowListener = (event: MessageEvent) => void;

class ControlledRuntimePort {
  readonly posted: unknown[] = [];
  readonly disconnect = vi.fn();
  private messageListener: ((message: unknown) => void) | undefined;
  private disconnectListener: (() => void) | undefined;

  readonly onMessage = {
    addListener: (listener: (message: unknown) => void) => { this.messageListener = listener; },
  };
  readonly onDisconnect = {
    addListener: (listener: () => void) => { this.disconnectListener = listener; },
  };

  postMessage(message: unknown): void { this.posted.push(message); }
  receive(message: unknown): void { this.messageListener?.(message); }
  drop(): void { this.disconnectListener?.(); }
}

function page(origin = "https://shop.example") {
  let listener: WindowListener | undefined;
  const postMessage = vi.fn();
  const window = {
    location: { origin },
    addEventListener: vi.fn((_type: "message", next: WindowListener) => { listener = next; }),
    postMessage,
  };
  return {
    window,
    postMessage,
    emit(data: unknown, eventOrigin = origin, source: unknown = window) {
      const event = new MessageEvent("message", { data, origin: eventOrigin });
      Object.defineProperty(event, "source", { value: source });
      listener?.(event);
    },
  };
}

function subscribe(subscriptionId: number, protocol = 1) {
  return {
    source: "escpost-page",
    kind: "subscribe",
    subscriptionId,
    op: "printers.events",
    protocol,
  };
}

function unsubscribe(subscriptionId: number) {
  return { source: "escpost-page", kind: "unsubscribe", subscriptionId };
}

async function flush(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

test("multiplexes page subscription ids over one runtime port and closes it after final unsubscribe", () => {
  // Break caught: opening a runtime port per id or retaining the final port
  // violates the content-document ownership and leaks worker/SSE resources.
  const fixture = page();
  const port = new ControlledRuntimePort();
  const runtime = { sendMessage: vi.fn(), connect: vi.fn(() => port) };
  installRelay(fixture.window, runtime);

  fixture.emit(subscribe(17));
  fixture.emit(subscribe(18));
  expect(runtime.connect).toHaveBeenCalledTimes(1);
  expect(runtime.connect).toHaveBeenCalledWith({ name: "escpost-printers" });
  expect(port.posted).toEqual([
    { kind: "subscribe", subscriptionId: 17, protocol: 1 },
    { kind: "subscribe", subscriptionId: 18, protocol: 1 },
  ]);

  port.receive({ kind: "snapshot", subscriptionId: 17, data: { printers: [] } });
  expect(fixture.postMessage).toHaveBeenCalledWith(
    { source: "escpost-extension", kind: "snapshot", subscriptionId: 17, data: { printers: [] } },
    "https://shop.example",
  );

  fixture.emit(unsubscribe(17));
  expect(port.posted.at(-1)).toEqual({ kind: "unsubscribe", subscriptionId: 17 });
  expect(port.disconnect).not.toHaveBeenCalled();
  fixture.emit(unsubscribe(18));
  expect(port.posted.at(-1)).toEqual({ kind: "unsubscribe", subscriptionId: 18 });
  expect(port.disconnect).toHaveBeenCalledOnce();
});

test("reconnects a lost runtime port and reissues every still-active id", async () => {
  // Break caught: treating MV3 worker suspension as cancellation leaves live
  // SDK callbacks permanently detached from the replacement worker port.
  const fixture = page();
  const first = new ControlledRuntimePort();
  const second = new ControlledRuntimePort();
  const ports = [first, second];
  const runtime = { sendMessage: vi.fn(), connect: vi.fn(() => ports.shift() ?? second) };
  installRelay(fixture.window, runtime);
  fixture.emit(subscribe(17));
  fixture.emit(subscribe(18));

  first.drop();
  expect(runtime.connect).toHaveBeenCalledTimes(1);
  await flush();

  expect(runtime.connect).toHaveBeenCalledTimes(2);
  expect(second.posted).toEqual([
    { kind: "subscribe", subscriptionId: 17, protocol: 1 },
    { kind: "subscribe", subscriptionId: 18, protocol: 1 },
  ]);
});

test("does not reconnect when the final id is removed before the reconnect microtask", async () => {
  // Break caught: a queued reconnect racing final unsubscribe can recreate a
  // port after the content document has released all stream ownership.
  const fixture = page();
  const first = new ControlledRuntimePort();
  const runtime = { sendMessage: vi.fn(), connect: vi.fn(() => first) };
  installRelay(fixture.window, runtime);
  fixture.emit(subscribe(17));

  first.drop();
  fixture.emit(unsubscribe(17));
  await flush();

  expect(runtime.connect).toHaveBeenCalledTimes(1);
});

test("rejects spoofed or malformed page subscriptions before opening a port", () => {
  // Break caught: accepting a foreign window/origin or widened subscription
  // message lets unrelated page code acquire a long-lived privileged channel.
  const fixture = page();
  const runtime = { sendMessage: vi.fn(), connect: vi.fn(() => new ControlledRuntimePort()) };
  installRelay(fixture.window, runtime);

  fixture.emit(subscribe(17), "https://evil.example");
  fixture.emit(subscribe(18), "https://shop.example", {});
  fixture.emit({ ...subscribe(19), unexpected: true });
  fixture.emit(subscribe(20, 2));

  expect(runtime.connect).not.toHaveBeenCalled();
  expect(fixture.postMessage).toHaveBeenCalledTimes(1);
  expect(fixture.postMessage).toHaveBeenCalledWith(
    { source: "escpost-extension", kind: "failure", subscriptionId: 20, error: expect.objectContaining({ code: "PROTOCOL_MISMATCH" }) },
    "https://shop.example",
  );
});

test("forwards only exact worker stream messages for an active id and current origin", () => {
  // Break caught: trusting malformed/unknown worker output or posting after
  // navigation crosses the isolated-world validation and origin boundaries.
  const fixture = page();
  const port = new ControlledRuntimePort();
  installRelay(fixture.window, { sendMessage: vi.fn(), connect: vi.fn(() => port) });
  fixture.emit(subscribe(17));

  port.receive({ kind: "snapshot", subscriptionId: 99, data: {} });
  port.receive({ kind: "snapshot", subscriptionId: 17 });
  port.receive({ kind: "snapshot", subscriptionId: 17, data: {}, extra: true });
  port.receive({ kind: "failure", subscriptionId: 17, error: { code: "NOT_A_CODE", message: "nope" } });
  expect(fixture.postMessage).not.toHaveBeenCalled();

  port.receive({ kind: "failure", subscriptionId: 17, error: { code: "DAEMON_UNAVAILABLE", message: "lost" } });
  expect(fixture.postMessage).toHaveBeenCalledWith(
    { source: "escpost-extension", kind: "failure", subscriptionId: 17, error: { code: "DAEMON_UNAVAILABLE", message: "lost" } },
    "https://shop.example",
  );

  fixture.window.location.origin = "https://after-navigation.example";
  port.receive({ kind: "snapshot", subscriptionId: 17, data: { printers: [] } });
  expect(fixture.postMessage).toHaveBeenCalledTimes(1);
  expect(port.disconnect).toHaveBeenCalledOnce();
});
