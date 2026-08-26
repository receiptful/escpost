import { afterEach, describe, expect, jest, test } from "bun:test";
import { openPrinterInventoryStream } from "./printer-inventory-stream";

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
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }

  close() { this.closed = true; }

  emit(name: string, payload?: unknown) {
    const event = payload === undefined ? new Event(name) : new MessageEvent(name, { data: JSON.stringify(payload) });
    for (const handler of this.listeners.get(name) ?? []) handler(event);
  }

  emitRaw(name: string, data: string) {
    for (const handler of this.listeners.get(name) ?? []) handler(new MessageEvent(name, { data }));
  }
}

const originalEventSource = globalThis.EventSource;

afterEach(() => { globalThis.EventSource = originalEventSource; });

describe("openPrinterInventoryStream", () => {
  test("uses native inventory messages and closes its source", () => {
    FakeEventSource.instances = [];
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    const onSnapshot = jest.fn();
    const onError = jest.fn();
    const snapshot = { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [] };

    const close = openPrinterInventoryStream({ onSnapshot, onError });
    const source = FakeEventSource.instances[0]!;
    source.emit("message", snapshot);

    expect(source.url).toBe("/api/printers/list/events");
    expect(onSnapshot).toHaveBeenCalledWith(snapshot);
    expect(onError).not.toHaveBeenCalled();
    close();
    expect(source.closed).toBe(true);
  });

  test("reports malformed JSON without closing automatic reconnection", () => {
    FakeEventSource.instances = [];
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    const onError = jest.fn();
    openPrinterInventoryStream({ onSnapshot: jest.fn(), onError });
    const source = FakeEventSource.instances[0]!;

    source.emitRaw("message", "not json");

    expect(onError).toHaveBeenCalledWith(new Error("The server sent an invalid printer inventory."));
    expect(source.closed).toBe(false);
  });

  test("reports native transport errors without closing automatic reconnection", () => {
    FakeEventSource.instances = [];
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    const onError = jest.fn();
    openPrinterInventoryStream({ onSnapshot: jest.fn(), onError });
    const source = FakeEventSource.instances[0]!;

    source.emit("error");

    expect(onError).toHaveBeenCalledWith(new Error("Printer monitoring disconnected; retrying automatically."));
    expect(source.closed).toBe(false);
  });
});
