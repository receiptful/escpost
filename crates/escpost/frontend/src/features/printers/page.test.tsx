import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/preact";
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

function pageFetch(input: RequestInfo | URL, init?: RequestInit) {
  const path = String(input);
  const method = init?.method ?? "GET";
  if (method === "GET" && path === "/api/printers/discover/networks") {
    return Promise.resolve(new Response(JSON.stringify({
      networks: [{ subnet: "10.0.0.0/24", interface: "eth0", hosts: 253 }],
      skipped: [], default_port: 9100, default_timeout_ms: 1000,
    }), { headers: { "content-type": "application/json" } }));
  }
  if (method === "GET" && path === "/api/profiles/list") {
    return Promise.resolve(new Response(JSON.stringify({ profiles: [] }), { headers: { "content-type": "application/json" } }));
  }
  if (method === "POST" && path === "/api/printers/add") {
    const body = JSON.parse(String(init?.body)) as { name: string; profile: string | null; connection: { type: "usb" | "network" } };
    return Promise.resolve(new Response(JSON.stringify({ name: body.name, transport: body.connection.type, profile: body.profile, warnings: [] }), {
      status: 201, headers: { "content-type": "application/json" },
    }));
  }
  return Promise.reject(new Error(`unexpected ${method} ${path}`));
}

function renderPage() {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  fetchMock = jest.fn(pageFetch);
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
  fetchMock = jest.fn(pageFetch);
  globalThis.fetch = fetchMock as unknown as typeof globalThis.fetch;
  render(<ServerStatusProvider><PrinterInventoryProvider><AppDataProvider><ToggleablePrintersPage /></AppDataProvider></PrinterInventoryProvider></ServerStatusProvider>);
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("message", { virtual_printer: null, jobs_processed: 0, config_path: "/tmp/printers.toml" }));
  act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", inventory([])));
}

afterEach(() => { cleanup(); globalThis.EventSource = originalEventSource; globalThis.fetch = originalFetch; });

describe("PrintersPage", () => {
  test("renders every configured-printer fact in both responsive layouts", () => {
    renderPage();
    act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", inventory([
      { name: "Kitchen", transport: "network", availability: "connected", profile: "REFERENCE", connection: { type: "network", host: "10.0.0.8", port: 9100 } },
      { name: "Counter", transport: "usb", availability: "unavailable", profile: null, connection: { type: "usb", vendor_id: 1046, product_id: 20497, bus: "003", address: 4, manufacturer: null, product: null, serial_number: "B120300001", interface_number: 0, out_endpoints: [1], in_endpoints: [] } },
    ])));
    const rows = screen.getAllByRole("row").filter((row) => row.querySelector("td"));
    const cards = [...document.querySelectorAll("article")];
    const expected = [
      ["Kitchen", "Connected", "REFERENCE", "IP", "10.0.0.8:9100"],
      ["Counter", "Unavailable", "No profile", "USB", "USB 0416:5011, bus 003 address 4, serial B120300001, interface 0"],
    ];
    expect(rows).toHaveLength(2);
    expect(cards).toHaveLength(2);
    for (const [index, facts] of expected.entries()) {
      for (const fact of facts) {
        expect(within(rows[index]!).getByText(fact)).toBeTruthy();
        expect(within(cards[index]!).getByText(fact)).toBeTruthy();
      }
    }
  });

  test("keeps desktop printer columns fixed when their content lengths differ", () => {
    renderPage();
    act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", inventory([
      {
        name: "A-printer-name-that-is-much-longer-than-the-other-values",
        transport: "network",
        availability: "connected",
        profile: "AN-EXCEPTIONALLY-LONG-PROFILE-NAME",
        connection: { type: "network", host: "printer-with-a-long-hostname.example.internal", port: 9100 },
      },
    ])));

    const table = screen.getByRole("table") as HTMLTableElement;
    expect(table.style.tableLayout).toBe("fixed");
    expect([...table.querySelectorAll("col")].map((column) => (column as HTMLElement).style.width)).toEqual([
      "20%",
      "15%",
      "20%",
      "45%",
    ]);
    for (const cell of table.querySelectorAll("tbody td")) {
      expect((cell as HTMLElement).style.overflowWrap).toBe("anywhere");
    }
  });

  test("uses monitor states, complete inventory snapshots, warning, and timestamp", () => {
    renderPage();
    expect(screen.getByText("Connecting to printer monitor…")).toBeTruthy();
    const updatedAt = "2026-08-27T11:36:50.502523006Z";
    act(() => FakeEventSource.forUrl("/api/printers/list/events")?.emit("message", {
      ...inventory([
        { name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } },
      ], "Monitor is catching up"),
      updated_at: updatedAt,
    }));
    expect(screen.getAllByText("Kitchen")).toHaveLength(2);
    expect(screen.getByText("Monitor is catching up")).toBeTruthy();
    const time = screen.getByText("Last updated").querySelector("time");
    expect(time?.getAttribute("dateTime")).toBe(updatedAt);
    expect(time?.textContent).not.toBe(updatedAt);
    expect(time?.textContent).toContain("2026");
    expect(time?.textContent).not.toContain("T");
    expect(time?.textContent).not.toContain("502523006");
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
    await act(async () => { for (let turn = 0; turn < 12; turn += 1) await Promise.resolve(); });
    expect(screen.queryByRole("button", { name: "Add printer" })).toBeNull();
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

  test("assembles discovery controls in one lifecycle bar", async () => {
    renderPage();
    expect(screen.getByRole("heading", { name: "Printer Discovery" })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "Configured Printers" })).toBeTruthy();
    const options = screen.getByRole("button", { name: "Scan options" });
    const add = screen.getByRole("button", { name: "Add IP printer manually" });
    const scan = await screen.findByRole("button", { name: "Scan" });
    expect(options.getAttribute("aria-expanded")).toBe("false");
    expect(add.closest("footer")).toBe(scan.closest("footer"));
    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
    await waitFor(() => expect(scan.hasAttribute("disabled")).toBe(false));
    fireEvent.click(scan);
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();
    act(() => FakeEventSource.forUrlPrefix("/api/printers/discover")?.emit("completed", {}));
    expect(screen.getByRole("button", { name: "Rescan" })).toBeTruthy();
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
    fireEvent.click(screen.getByRole("button", { name: "Add 10.0.0.45:9100" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "Kitchen" } });
    fireEvent.click(screen.getByRole("button", { name: "Add printer" }));
    await waitFor(() => expect(fetchMock.mock.calls.map(([input]) => String(input))).toContain("/api/printers/add"));
    await act(async () => { for (let turn = 0; turn < 12; turn += 1) await Promise.resolve(); });
    expect(screen.queryByRole("button", { name: "Add 10.0.0.45:9100" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Leave printers" }));
    fireEvent.click(screen.getByRole("button", { name: "Leave printers" }));
    expect(screen.queryByRole("button", { name: "Add 10.0.0.45:9100" })).toBeNull();
    expect(screen.getByText("1 printer found (0 new)")).toBeTruthy();
  });

  test("keeps the chosen scan scope when the printers page remounts", async () => {
    renderToggleablePage();
    fireEvent.click(screen.getByRole("button", { name: "Scan options" }));
    const port = await screen.findByLabelText("RAW TCP port");
    fireEvent.input(port, { target: { value: "9101" } });
    const scan = screen.getByRole("button", { name: "Scan" });
    await waitFor(() => expect(scan.hasAttribute("disabled")).toBe(false));
    fireEvent.click(scan);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByRole("button", { name: "Leave printers" }));
    fireEvent.click(screen.getByRole("button", { name: "Leave printers" }));
    const rescan = screen.getByRole("button", { name: "Rescan" });
    await waitFor(() => expect(rescan.hasAttribute("disabled")).toBe(false));
    fireEvent.click(rescan);
    const streams = FakeEventSource.instances.filter((source) => source.url.startsWith("/api/printers/discover"));
    expect(streams.map((source) => source.url)).toEqual([
      "/api/printers/discover?port=9101&timeout=1000",
      "/api/printers/discover?port=9101&timeout=1000",
    ]);
  });
});
