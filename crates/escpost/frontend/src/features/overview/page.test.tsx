import { afterEach, describe, expect, test } from "bun:test";
import { act, cleanup, render, screen, within } from "@testing-library/preact";
import { AppDataProvider } from "../../app/data";
import { PrinterInventoryProvider } from "../../app/printer-inventory-data";
import { ServerStatusProvider } from "../../app/server-status-data";
import { OverviewPage } from "./page";

class FakeEventSource {
  static instances: FakeEventSource[] = [];
  private listeners = new Map<string, ((event: Event) => void)[]>();
  constructor(readonly url: string) { FakeEventSource.instances.push(this); }
  addEventListener(name: string, handler: (event: Event) => void) { this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]); }
  close() {}
  emit(name: string, data: unknown) { for (const handler of this.listeners.get(name) ?? []) handler(new MessageEvent(name, { data: JSON.stringify(data) })); }
  static forUrl(url: string) { return FakeEventSource.instances.find((source) => source.url === url); }
}
const originalEventSource = globalThis.EventSource;
afterEach(() => { cleanup(); globalThis.EventSource = originalEventSource; });

function renderOverview(printers: unknown[] = [], warning: string | null = null) {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  render(<ServerStatusProvider><PrinterInventoryProvider><AppDataProvider><OverviewPage /></AppDataProvider></PrinterInventoryProvider></ServerStatusProvider>);
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("message", { virtual_printer: { state: "receiving", address: "127.0.0.1:9100" }, jobs_processed: 7, config_path: "" }));
  act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", { updated_at: "2026-08-26T14:32:10Z", warning, printers }));
}

describe("OverviewPage", () => {
  test("derives printer counts from the inventory snapshot", () => {
    renderOverview([
      { name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } },
      { name: "Counter", transport: "network", availability: "unavailable", profile: null, connection: { type: "network", host: "10.0.0.9", port: 9100 } },
    ]);
    const printers = screen.getByRole("region", { name: "Printers" });
    expect(within(printers).getByText("2 configured")).toBeTruthy();
    expect(within(printers).getByText("1 connected")).toBeTruthy();
    expect(within(printers).getByText("1 unavailable")).toBeTruthy();
    expect(screen.getByText("Receiving")).toBeTruthy();
  });

  test("shows the backend inventory warning without hiding facts", () => {
    renderOverview([], "Monitor is catching up");
    expect(screen.getByText("0 configured")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("Monitor is catching up");
  });
});
