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

  listenerNames() { return [...this.listeners.keys()]; }

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
    expect(source.listenerNames()).toEqual(["message", "error"]);
    expect(source.listenerNames()).not.toContain("printer");
    expect(source.listenerNames()).not.toContain("status");
    expect(onSnapshot).toHaveBeenCalledWith(snapshot);
    expect(onError).not.toHaveBeenCalled();
    close();
    expect(source.closed).toBe(true);
  });

  test("rejects every malformed snapshot shape, retains no bad data, and recovers with a valid message", () => {
    FakeEventSource.instances = [];
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    const onSnapshot = jest.fn();
    const onError = jest.fn();
    openPrinterInventoryStream({ onSnapshot, onError });
    const source = FakeEventSource.instances[0]!;
    const networkPrinter = {
      name: "Kitchen", transport: "network", availability: "connected", profile: null,
      connection: { type: "network", host: "10.0.0.8", port: 9100 },
    };
    const usbPrinter = {
      name: "Counter", transport: "usb", availability: "unavailable", profile: "REFERENCE",
      connection: {
        type: "usb", vendor_id: 1046, product_id: 20497, bus: "003", address: 7,
        manufacturer: null, product: "POS-58", serial_number: null, interface_number: 0,
        out_endpoints: [1], in_endpoints: [],
      },
    };
    const valid = { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [networkPrinter, usbPrinter] };

    for (const invalid of [
      null,
      [],
      { updated_at: 0, warning: null, printers: [] },
      { updated_at: "not-a-date", warning: null, printers: [] },
      { updated_at: "2026-08-26T14:32:10Z", warning: 3, printers: [] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: {} },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...networkPrinter, name: 3 }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...networkPrinter, profile: false }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...networkPrinter, transport: "usb" }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...networkPrinter, availability: "offline" }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...networkPrinter, connection: { type: "network", host: 3, port: "9100" } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, vendor_id: "1046" } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, out_endpoints: ["1"] } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...networkPrinter, connection: { type: "serial" } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...networkPrinter, connection: { ...networkPrinter.connection, port: -1 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...networkPrinter, connection: { ...networkPrinter.connection, port: 1.5 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...networkPrinter, connection: { ...networkPrinter.connection, port: 65_536 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, vendor_id: -1 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, product_id: 65_536 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, product_id: -1 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, product_id: 1.5 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, vendor_id: 1.5 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, address: 256 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, address: -1 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, address: 1.5 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, interface_number: -1 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, interface_number: 256 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, interface_number: 1.5 } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, out_endpoints: [1.5] } }] },
      { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: [{ ...usbPrinter, connection: { ...usbPrinter.connection, in_endpoints: [256] } }] },
    ]) source.emit("message", invalid);

    expect(onSnapshot).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledTimes(30);
    source.emit("message", valid);
    expect(onSnapshot).toHaveBeenCalledWith(valid);
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
