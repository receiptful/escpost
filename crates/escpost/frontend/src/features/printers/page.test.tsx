import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { useState } from "preact/hooks";
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
  static forUrlPrefix(prefix: string) { return FakeEventSource.instances.find((source) => source.url.startsWith(prefix)); }
}
const originalEventSource = globalThis.EventSource;
const originalFetch = globalThis.fetch;
let fetchMock: ReturnType<typeof jest.fn>;
const inventory = (printers: unknown[], warning: string | null = null) => ({ updated_at: "2026-08-26T14:32:10Z", warning, printers });

function renderPage() {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  fetchMock = jest.fn((input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    const body = path === "/api/printers/add"
      ? { name: JSON.parse(String(init?.body)).name, transport: "network", profile: null, warnings: [] }
      : path === "/api/printers/discover/networks"
        ? { networks: [{ subnet: "10.0.0.0/24", interface: "eth0", hosts: 253 }], skipped: [], default_port: 9100, default_timeout_ms: 1000 }
        : { profiles: [] };
    return Promise.resolve(new Response(JSON.stringify(body), { status: path === "/api/printers/add" ? 201 : 200, headers: { "content-type": "application/json" } }));
  });
  globalThis.fetch = fetchMock as unknown as typeof globalThis.fetch;
  render(<ServerStatusProvider><PrinterInventoryProvider><AppDataProvider><PrintersPage /></AppDataProvider></PrinterInventoryProvider></ServerStatusProvider>);
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("message", { virtual_printer: null, jobs_processed: 0, config_path: "/tmp/printers.toml" }));
}

function ToggleablePrintersPage() {
  const [visible, setVisible] = useState(true);
  return <>
    <button type="button" onClick={() => setVisible((current) => !current)}>Leave printers</button>
    {visible && <PrintersPage />}
  </>;
}

function renderToggleablePage() {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  fetchMock = jest.fn((input: RequestInfo | URL) => {
    const body = String(input) === "/api/printers/discover/networks"
      ? { networks: [{ subnet: "10.0.0.0/24", interface: "eth0", hosts: 253 }], skipped: [], default_port: 9100, default_timeout_ms: 1000 }
      : { profiles: [] };
    return Promise.resolve(new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } }));
  });
  globalThis.fetch = fetchMock as unknown as typeof globalThis.fetch;
  render(<ServerStatusProvider><PrinterInventoryProvider><AppDataProvider><ToggleablePrintersPage /></AppDataProvider></PrinterInventoryProvider></ServerStatusProvider>);
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("message", { virtual_printer: null, jobs_processed: 0, config_path: "/tmp/printers.toml" }));
  act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", inventory([])));
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

  test("registering a printer waits for its next inventory event and never GETs the list", async () => {
    renderPage();
    act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", inventory([])));
    fireEvent.click(screen.getByRole("button", { name: "Add IP printer manually" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "Kitchen" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.0.8" } });
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));
    await waitFor(() => expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain("/api/printers/add"));
    expect(screen.queryByText("Kitchen")).toBeNull();
    expect(fetchMock.mock.calls.map(([input]) => String(input))).not.toContain("/api/printers/list");
    const source = FakeEventSource.forUrl("/api/printers/list/events");
    expect(source).toBeTruthy();
    act(() => source?.emit("message", inventory([
      { name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } },
    ])));
    const [row, card] = screen.getAllByText("Kitchen");
    expect(row?.closest("tr")?.classList.contains("printer-row-found")).toBe(true);
    expect(card?.closest("article")?.classList.contains("printer-row-found")).toBe(true);
  });

  test("keeps discovery results after cancelling a scan", async () => {
    renderPage();
    act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", inventory([])));
    const scan = await screen.findByRole("button", { name: "Scan" });
    await waitFor(() => expect(scan.hasAttribute("disabled")).toBe(false));
    fireEvent.click(scan);
    const discovery = FakeEventSource.forUrlPrefix("/api/printers/discover")!;
    act(() => discovery.emit("printer", {
      transport: "network", configured_names: [], configured_profile: null, interface: "eth0",
      connection: { type: "network", host: "10.0.0.44", port: 9100 },
    }));
    expect(screen.getByText("10.0.0.44:9100")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Rescan" })).toBeTruthy();
    expect(screen.getByText("10.0.0.44:9100")).toBeTruthy();
  });

  test("keeps a running discovery result when the printers page remounts", async () => {
    renderToggleablePage();
    const scan = await screen.findByRole("button", { name: "Scan" });
    await waitFor(() => expect(scan.hasAttribute("disabled")).toBe(false));
    fireEvent.click(scan);
    act(() => FakeEventSource.forUrlPrefix("/api/printers/discover")?.emit("printer", {
      transport: "network", configured_names: [], configured_profile: null, interface: "eth0",
      connection: { type: "network", host: "10.0.0.45", port: 9100 },
    }));
    expect(screen.getByText("10.0.0.45:9100")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Leave printers" }));
    fireEvent.click(screen.getByRole("button", { name: "Leave printers" }));
    expect(screen.getByText("10.0.0.45:9100")).toBeTruthy();
  });
});
