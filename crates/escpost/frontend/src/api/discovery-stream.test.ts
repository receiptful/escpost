import { afterEach, describe, expect, jest, test } from "bun:test";
import { discoveryQueryString, openDiscoveryStream } from "./discovery-stream";

// Neither Bun's runtime nor the happy-dom registrator used by the test setup
// provides a global `EventSource`, so the stream is exercised against a
// small stand-in that records listeners and lets a test dispatch named
// events, the same way `client.test.ts` stands in for `fetch`.
class FakeEventSource {
  static instances: FakeEventSource[] = [];
  readonly url: string;
  closed = false;
  private readonly listeners = new Map<string, ((event: Event) => void)[]>();

  constructor(url: string) {
    this.url = url;
    FakeEventSource.instances.push(this);
  }

  addEventListener(name: string, handler: (event: Event) => void) {
    const existing = this.listeners.get(name) ?? [];
    existing.push(handler);
    this.listeners.set(name, existing);
  }

  close() {
    this.closed = true;
  }

  emit(name: string, data?: unknown) {
    const event = data === undefined ? new Event(name) : new MessageEvent(name, { data: JSON.stringify(data) });
    for (const handler of this.listeners.get(name) ?? []) {
      handler(event);
    }
  }
}

const originalEventSource = globalThis.EventSource;

function installFakeEventSource() {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
}

afterEach(() => {
  globalThis.EventSource = originalEventSource;
});

function handlers() {
  return {
    onPrepared: jest.fn(),
    onPrinter: jest.fn(),
    onProgress: jest.fn(),
    onUsbFailure: jest.fn(),
    onCompleted: jest.fn(),
    onError: jest.fn(),
  };
}

describe("discoveryQueryString", () => {
  test("omits transport when both transports are selected", () => {
    const query = discoveryQueryString({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 });
    expect(query).toBe("port=9100&timeout=1000");
  });

  test("sends transport only when exactly one transport is selected", () => {
    expect(discoveryQueryString({ usb: false, network: true, subnets: [], port: 9100, timeoutMs: 1000 })).toBe(
      "transport=network&port=9100&timeout=1000",
    );
  });

  // `printers discover --transport usb` refuses every network flag, and the
  // endpoint builds the same arguments, so a query that restates the defaults
  // is rejected with 400 rather than ignored. The panel keeps its port and
  // timeout fields filled while Network is unchecked, so the values reach
  // this function and have to be dropped here.
  test("drops the port and timeout from a USB-only scan, which the shared layer refuses to accept alongside them", () => {
    expect(discoveryQueryString({ usb: true, network: false, subnets: ["10.42.0.0/24"], port: 9100, timeoutMs: 1000 })).toBe(
      "transport=usb",
    );
  });

  test("repeats subnet once per network and sends the port and timeout beside them", () => {
    const query = discoveryQueryString({
      usb: true,
      network: true,
      subnets: ["10.42.0.0/24", "10.43.0.0/24"],
      port: 9100,
      timeoutMs: 500,
    });
    expect(query).toBe("subnet=10.42.0.0%2F24&subnet=10.43.0.0%2F24&port=9100&timeout=500");
  });
});

describe("openDiscoveryStream", () => {
  test("opens the stream at the discovery endpoint with the query string", () => {
    installFakeEventSource();
    openDiscoveryStream({ usb: true, network: false, subnets: [], port: 9100, timeoutMs: 1000 }, handlers());

    expect(FakeEventSource.instances).toHaveLength(1);
    expect(FakeEventSource.instances[0]?.url).toBe("/api/printers/discover?transport=usb");
  });

  test("dispatches each named event's parsed payload to its handler", () => {
    installFakeEventSource();
    const callbacks = handlers();
    openDiscoveryStream({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 }, callbacks);
    const source = FakeEventSource.instances[0]!;

    const prepared = { targets: [], skipped: [], total_probes: 4 };
    source.emit("prepared", prepared);
    expect(callbacks.onPrepared).toHaveBeenCalledWith(prepared);

    const printer = {
      transport: "network",
      configured_names: [],
      configured_profile: null,
      connection: { type: "network", host: "10.42.0.71", port: 9100 },
    };
    source.emit("printer", printer);
    expect(callbacks.onPrinter).toHaveBeenCalledWith(printer);

    const progress = { completed: 1, total: 4 };
    source.emit("progress", progress);
    expect(callbacks.onProgress).toHaveBeenCalledWith(progress);

    const failure = { vendor_id: 1, product_id: 2, stage: "open_device", reason: "denied", permission_denied: true, can_grant_usb_permissions: true };
    source.emit("usb_failure", failure);
    expect(callbacks.onUsbFailure).toHaveBeenCalledWith(failure);
  });

  test("closes the stream and reports completion on a completed event", () => {
    installFakeEventSource();
    const callbacks = handlers();
    openDiscoveryStream({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 }, callbacks);
    const source = FakeEventSource.instances[0]!;

    source.emit("completed", {});

    expect(source.closed).toBe(true);
    expect(callbacks.onCompleted).toHaveBeenCalledTimes(1);
  });

  test("closes the stream and surfaces the server's message on a server-sent error event", () => {
    installFakeEventSource();
    const callbacks = handlers();
    openDiscoveryStream({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 }, callbacks);
    const source = FakeEventSource.instances[0]!;

    source.emit("error", { message: "The configuration file could not be read." });

    expect(source.closed).toBe(true);
    expect(callbacks.onError).toHaveBeenCalledWith("The configuration file could not be read.");
  });

  test("closes the stream and reports a generic message on a connection-level error", () => {
    installFakeEventSource();
    const callbacks = handlers();
    openDiscoveryStream({ usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 }, callbacks);
    const source = FakeEventSource.instances[0]!;

    source.emit("error");

    expect(source.closed).toBe(true);
    expect(callbacks.onError).toHaveBeenCalledWith("The discovery stream ended unexpectedly.");
  });

  test("the returned closer cancels the scan without invoking a handler", () => {
    installFakeEventSource();
    const callbacks = handlers();
    const close = openDiscoveryStream(
      { usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 },
      callbacks,
    );
    const source = FakeEventSource.instances[0]!;

    close();

    expect(source.closed).toBe(true);
    expect(callbacks.onCompleted).not.toHaveBeenCalled();
    expect(callbacks.onError).not.toHaveBeenCalled();
  });
});
