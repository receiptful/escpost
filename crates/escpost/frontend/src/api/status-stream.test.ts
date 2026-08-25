import { afterEach, describe, expect, jest, test } from "bun:test";
import { openServerStatusStream } from "./status-stream";

// Neither Bun nor happy-dom provides EventSource. This small stand-in drives
// the adapter through the same named events that the browser receives.
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
    this.emitRaw(name, data === undefined ? undefined : JSON.stringify(data));
  }

  emitRaw(name: string, data?: string) {
    const event = data === undefined ? new Event(name) : new MessageEvent(name, { data });
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

describe("openServerStatusStream", () => {
  test("opens the persistent server-status endpoint and dispatches valid snapshots", () => {
    installFakeEventSource();
    const handlers = { onStatus: jest.fn(), onError: jest.fn() };
    openServerStatusStream(handlers);
    const source = FakeEventSource.instances[0]!;
    const snapshot = {
      virtual_printer: null,
      jobs_processed: 3,
      config_path: "/tmp/printers.toml",
    };

    source.emit("status", snapshot);

    expect(source.url).toBe("/api/status/events");
    expect(handlers.onStatus).toHaveBeenCalledWith(snapshot);
  });

  test("reports connection errors without closing automatic reconnection", () => {
    installFakeEventSource();
    const handlers = { onStatus: jest.fn(), onError: jest.fn() };
    openServerStatusStream(handlers);
    const source = FakeEventSource.instances[0]!;

    source.emit("error");

    expect(handlers.onError).toHaveBeenCalledTimes(1);
    expect(source.closed).toBe(false);
  });

  test("rejects malformed status data and accepts the next valid snapshot", () => {
    installFakeEventSource();
    const handlers = { onStatus: jest.fn(), onError: jest.fn() };
    openServerStatusStream(handlers);
    const source = FakeEventSource.instances[0]!;

    source.emitRaw("status", "not json");
    source.emit("status", { virtual_printer: null, jobs_processed: 0, config_path: "" });

    expect(handlers.onError).toHaveBeenCalledTimes(1);
    expect(handlers.onStatus).toHaveBeenCalledTimes(1);
    expect(source.closed).toBe(false);
  });

  test("rejects every invalid public snapshot field without interrupting later snapshots", () => {
    installFakeEventSource();
    const handlers = { onStatus: jest.fn(), onError: jest.fn() };
    openServerStatusStream(handlers);
    const source = FakeEventSource.instances[0]!;

    for (const invalid of [
      { virtual_printer: { state: "printing", address: "127.0.0.1:9100" }, jobs_processed: 0, config_path: "/tmp/printers.toml" },
      { virtual_printer: { state: "ready", address: 9100 }, jobs_processed: 0, config_path: "/tmp/printers.toml" },
      { virtual_printer: null, jobs_processed: -1, config_path: "/tmp/printers.toml" },
      { virtual_printer: null, jobs_processed: Number.MAX_SAFE_INTEGER + 1, config_path: "/tmp/printers.toml" },
      { virtual_printer: null, jobs_processed: 0.5, config_path: "/tmp/printers.toml" },
      { virtual_printer: null, jobs_processed: 0, config_path: null },
    ]) {
      source.emit("status", invalid);
    }
    const snapshot = {
      virtual_printer: { state: "receiving" as const, address: "127.0.0.1:9100" },
      jobs_processed: 4,
      config_path: "/tmp/printers.toml",
    };
    source.emit("status", snapshot);

    expect(handlers.onError).toHaveBeenCalledTimes(6);
    expect(handlers.onStatus).toHaveBeenCalledWith(snapshot);
    expect(source.closed).toBe(false);
  });

  test("closes only when its returned teardown runs", () => {
    installFakeEventSource();
    const close = openServerStatusStream({ onStatus: jest.fn(), onError: jest.fn() });
    const source = FakeEventSource.instances[0]!;

    close();

    expect(source.closed).toBe(true);
  });
});
