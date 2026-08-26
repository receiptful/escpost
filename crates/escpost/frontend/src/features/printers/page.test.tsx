import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { AppDataProvider } from "../../app/data";
import { PrinterInventoryProvider } from "../../app/printer-inventory-data";
import { ServerStatusProvider } from "../../app/server-status-data";
import { PrintersPage } from "./page";

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
const originalFetch = globalThis.fetch;
let fetchMock: ReturnType<typeof jest.fn>;
const inventory = (printers: unknown[], warning: string | null = null) => ({ updated_at: "2026-08-26T14:32:10Z", warning, printers });

function renderPage() {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  fetchMock = jest.fn((input: RequestInfo | URL) => Promise.resolve(new Response(JSON.stringify(
    String(input) === "/api/printers/discover/networks" ? { networks: [], skipped: [], default_port: 9100, default_timeout_ms: 1000 } : { profiles: [] },
  ), { headers: { "content-type": "application/json" } })));
  globalThis.fetch = fetchMock as unknown as typeof globalThis.fetch;
  render(<ServerStatusProvider><PrinterInventoryProvider><AppDataProvider><PrintersPage /></AppDataProvider></PrinterInventoryProvider></ServerStatusProvider>);
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("message", { virtual_printer: null, jobs_processed: 0, config_path: "/tmp/printers.toml" }));
}

afterEach(() => { cleanup(); globalThis.EventSource = originalEventSource; globalThis.fetch = originalFetch; });

describe("PrintersPage", () => {
  test("uses monitor states, complete inventory snapshots, warning, and timestamp", () => {
    renderPage();
    expect(screen.getByText("Connecting to printer monitor…")).toBeTruthy();
    act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", inventory([
      { name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } },
    ], "Monitor is catching up")));
    expect(screen.getAllByText("Kitchen")).toHaveLength(2);
    expect(screen.getByText("Monitor is catching up")).toBeTruthy();
    expect(screen.getByText("Last updated").querySelector("time")?.getAttribute("dateTime")).toBe("2026-08-26T14:32:10Z");
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  test("registering a printer waits for its next inventory event and never GETs the list", () => {
    renderPage();
    act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", inventory([])));
    fireEvent.click(screen.getByRole("button", { name: "Add IP printer manually" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "Kitchen" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.0.8" } });
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));
    expect(fetchMock.mock.calls.map(([input]) => String(input))).not.toContain("/api/printers/list");
    expect(FakeEventSource.forUrl("/api/printers/list/events")).toBeTruthy();
  });
});
