import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, render, screen } from "@testing-library/preact";
import { PrinterInventoryProvider, usePrinterInventory } from "./printer-inventory-data";

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  closed = false;
  private readonly listeners = new Map<string, ((event: Event) => void)[]>();
  constructor(readonly url: string) { FakeEventSource.instances.push(this); }
  addEventListener(name: string, handler: (event: Event) => void) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }
  close() { this.closed = true; }
  emit(name: string, payload?: unknown) {
    const event = payload === undefined ? new Event(name) : new MessageEvent(name, { data: JSON.stringify(payload) });
    for (const handler of this.listeners.get(name) ?? []) handler(event);
  }
}

const originalEventSource = globalThis.EventSource;
const snapshot = (printers: unknown[], warning: string | null = null) => ({
  updated_at: "2026-08-26T14:32:10Z", warning, printers,
});
const kitchen = (availability: "connected" | "unavailable" = "connected") => ({
  name: "Kitchen", transport: "network", availability, profile: null,
  connection: { type: "network", host: "10.0.0.8", port: 9100 },
});

function Probe() {
  const resource = usePrinterInventory();
  return <p>{`${resource.phase}:${resource.snapshot?.printers.map((printer) => printer.name).join(",") ?? "null"}:${resource.error?.message ?? "none"}:${JSON.stringify(resource.printerFlashes)}:${resource.snapshot?.warning ?? "none"}`}</p>;
}

function renderProvider() {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  return render(<PrinterInventoryProvider><Probe /></PrinterInventoryProvider>);
}

afterEach(() => { cleanup(); jest.useRealTimers(); globalThis.EventSource = originalEventSource; });

describe("PrinterInventoryProvider", () => {
  test("keeps one app-lifetime stream and atomically replaces complete snapshots", () => {
    renderProvider();
    const source = FakeEventSource.instances[0]!;
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(screen.getByText("checking:null:none:{}:none")).toBeTruthy();
    act(() => source.emit("message", snapshot([kitchen()])));
    expect(screen.getByText("ready:Kitchen:none:{}:none")).toBeTruthy();
    act(() => source.emit("message", snapshot([])));
    expect(screen.getByText("ready::none:{}:none")).toBeTruthy();
  });

  test("retains the last snapshot on error, recovers, and preserves warnings", () => {
    renderProvider();
    const source = FakeEventSource.instances[0]!;
    act(() => source.emit("message", snapshot([kitchen()], "Monitor lagging")));
    act(() => source.emit("error"));
    expect(screen.getByText("disconnected:Kitchen:Printer monitoring disconnected; retrying automatically.:{}:Monitor lagging")).toBeTruthy();
    act(() => source.emit("message", snapshot([], "Backend warning")));
    expect(screen.getByText("ready::none:{}:Backend warning")).toBeTruthy();
  });

  test("retains the last good snapshot for a syntactically valid but malformed stream message", () => {
    renderProvider();
    const source = FakeEventSource.instances[0]!;
    act(() => source.emit("message", snapshot([kitchen()], "Monitor lagging")));
    act(() => source.emit("message", { updated_at: "2026-08-26T14:32:10Z", warning: null, printers: {} }));
    expect(screen.getByText("disconnected:Kitchen:The server sent an invalid printer inventory.:{}:Monitor lagging")).toBeTruthy();
    act(() => source.emit("message", snapshot([], null)));
    expect(screen.getByText("ready::none:{}:none")).toBeTruthy();
  });

  test("flashes new and recovered printers, and clears flashes after 1.2 seconds", () => {
    jest.useFakeTimers();
    renderProvider();
    const source = FakeEventSource.instances[0]!;
    act(() => source.emit("message", snapshot([])));
    act(() => source.emit("message", snapshot([kitchen("connected")] )));
    expect(screen.getByText("ready:Kitchen:none:{\"Kitchen\":\"found\"}:none")).toBeTruthy();
    act(() => source.emit("message", snapshot([kitchen("unavailable")] )));
    expect(screen.getByText("ready:Kitchen:none:{\"Kitchen\":\"lost\"}:none")).toBeTruthy();
    act(() => source.emit("message", snapshot([kitchen("connected")] )));
    expect(screen.getByText("ready:Kitchen:none:{\"Kitchen\":\"found\"}:none")).toBeTruthy();
    act(() => { jest.advanceTimersByTime(1_200); });
    expect(screen.getByText("ready:Kitchen:none:{}:none")).toBeTruthy();
  });

  test("closes its source on unmount", () => {
    const view = renderProvider();
    const source = FakeEventSource.instances[0]!;
    view.unmount();
    expect(source.closed).toBe(true);
  });
});
