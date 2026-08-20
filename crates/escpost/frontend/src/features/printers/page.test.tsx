import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { AppDataProvider } from "../../app/data";
import { PrintersPage } from "./page";

const status = { virtual_printer: null, jobs_processed: 0, config_path: "/tmp/printers.toml" };
const printer = {
  name: "Kitchen",
  transport: "network",
  availability: "connected",
  profile: "REFERENCE",
  connection: { type: "network", host: "10.0.0.8", port: 9100 },
};

const networks = {
  networks: [{ subnet: "10.42.0.0/24", interface: "enx0", hosts: 253 }],
  skipped: [],
  default_port: 9100,
  default_timeout_ms: 1000,
};

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

// Every endpoint the assembled page reaches for on its own, so a test only
// has to state the one it is actually about.
function fetchStub(printers: unknown = { printers: [] }) {
  return ((input: RequestInfo | URL) => {
    const url = String(input);
    if (url === "/api/status") return Promise.resolve(json(status));
    if (url === "/api/profiles/list") return Promise.resolve(json({ profiles: [] }));
    if (url === "/api/printers/discover/networks") return Promise.resolve(json(networks));
    return Promise.resolve(json(printers));
  }) as typeof globalThis.fetch;
}

// The scan is an `EventSource`, which neither Bun nor the happy-dom
// registrator provides, so the page is driven against the same stand-in
// `data.test.tsx` uses — reduced to the one fact these tests assert, which is
// the query the split button and the options panel produced.
class FakeEventSource {
  static urls: string[] = [];
  constructor(url: string) {
    FakeEventSource.urls.push(url);
  }
  addEventListener() {}
  close() {}
}

const originalEventSource = globalThis.EventSource;

function renderPage(fetch: typeof globalThis.fetch) {
  globalThis.fetch = fetch;
  return render(<AppDataProvider><PrintersPage /></AppDataProvider>);
}

// Steps `bun:test`'s fake clock forward and flushes the microtask queue, so
// the fetch chain a poll timer kicked off has settled before the next
// assertion runs. Copied from `data.test.tsx`, where the same six turns are
// what it takes to drain fetch -> json() -> .then -> .finally.
async function advanceTimers(milliseconds: number) {
  jest.advanceTimersByTime(milliseconds);
  for (let i = 0; i < 6; i += 1) {
    await Promise.resolve();
  }
}

function openMenu() {
  fireEvent.click(screen.getByRole("button", { name: "Discovery options" }));
}

afterEach(() => {
  cleanup();
  jest.useRealTimers();
  globalThis.EventSource = originalEventSource;
  FakeEventSource.urls = [];
});

describe("PrintersPage", () => {
  test("distinguishes initial loading, empty inventory, and initial API error", async () => {
    let resolvePrinters!: (response: Response) => void;
    renderPage(((input: RequestInfo | URL) => String(input) === "/api/status"
      ? Promise.resolve(json(status))
      : String(input) === "/api/profiles/list"
        ? Promise.resolve(json({ profiles: [] }))
        : new Promise<Response>((resolve) => { resolvePrinters = resolve; })) as typeof globalThis.fetch);
    expect(screen.getByText("Loading printers…")).toBeTruthy();
    await act(async () => { resolvePrinters(json({ printers: [] })); });
    expect(await screen.findByText("No printers configured.")).toBeTruthy();

    cleanup();
    renderPage(((input: RequestInfo | URL) => String(input) === "/api/status"
      ? Promise.resolve(json(status))
      : String(input) === "/api/profiles/list"
        ? Promise.resolve(json({ profiles: [] }))
        : Promise.resolve(json({ error: { code: "printer_inventory_unavailable", message: "Printer inventory is unavailable." } }, 500))) as typeof globalThis.fetch);
    expect(await screen.findByText("Printer inventory is unavailable.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });

  test("replaces cached printer facts after automatic refresh and retains them after a failed refresh", async () => {
    jest.useFakeTimers();
    const inventories = [
      json({ printers: [printer] }),
      json({ printers: [{ ...printer, name: "Bar" }] }),
      json({ error: { code: "printer_inventory_unavailable", message: "Printer inventory is unavailable." } }, 500),
    ];
    renderPage(((input: RequestInfo | URL) => String(input) === "/api/status"
      ? Promise.resolve(json(status))
      : String(input) === "/api/profiles/list"
        ? Promise.resolve(json({ profiles: [] }))
        : Promise.resolve(inventories.shift()!)) as typeof globalThis.fetch);
    expect(await screen.findAllByText("Kitchen")).toHaveLength(2);

    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
    await act(async () => {
      jest.advanceTimersByTime(10_000);
      await Promise.resolve();
    });
    expect(await screen.findAllByText("Bar")).toHaveLength(2);

    await act(async () => {
      jest.advanceTimersByTime(10_000);
      await Promise.resolve();
    });
    expect(await screen.findByText("Showing cached printer data. Printer inventory is unavailable.")).toBeTruthy();
    expect(screen.getAllByText("Bar")).toHaveLength(2);
  });

  test("renders matching desktop-table and mobile-card printer facts", async () => {
    renderPage(((input: RequestInfo | URL) => String(input) === "/api/status"
      ? Promise.resolve(json(status))
      : String(input) === "/api/profiles/list"
        ? Promise.resolve(json({ profiles: [] }))
        : Promise.resolve(json({ printers: [printer] }))) as typeof globalThis.fetch);
    expect(await screen.findAllByText("Kitchen")).toHaveLength(2);
    expect(screen.getAllByText("Connected")).toHaveLength(2);
    expect(screen.getAllByText("Network")).toHaveLength(2);
    expect(screen.getAllByText("REFERENCE")).toHaveLength(2);
    expect(screen.getAllByText("10.0.0.8:9100")).toHaveLength(2);
  });

  test("the header offers discovery, scan options, and manual add, and no refresh button", async () => {
    renderPage(fetchStub());
    await act(async () => {});

    expect(screen.getByRole("button", { name: "Discover printers" })).toBeTruthy();
    expect(screen.queryByRole("menuitem", { name: "Scan options…" })).toBeNull();

    openMenu();
    expect(screen.getByRole("menuitem", { name: "Scan options…" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Add network printer manually" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
  });

  test("Discover printers scans with the current settings, and Cancel discards the scan", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());
    await act(async () => {});

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Discover printers" })); });
    expect(FakeEventSource.urls).toEqual(["/api/printers/discover?port=9100&timeout=1000"]);
    expect(screen.getByRole("heading", { name: "Discovering printers" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Rescan" })).toBeTruthy();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Cancel" })); });
    expect(screen.queryByRole("heading", { name: "Discovering printers" })).toBeNull();
    expect(screen.getByRole("button", { name: "Discover printers" })).toBeTruthy();
  });

  test("Scan options starts the scan it configured, and the next scan reuses those settings", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());
    await act(async () => {});

    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Scan options…" }));
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9101" } });

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Start scan" })); });
    expect(FakeEventSource.urls).toEqual(["/api/printers/discover?port=9101&timeout=1000"]);
    expect(screen.queryByRole("heading", { name: "Scan options" })).toBeNull();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Rescan" })); });
    expect(FakeEventSource.urls[1]).toBe("/api/printers/discover?port=9101&timeout=1000");
  });

  test("a manually registered printer lands in the inventory with the found highlight", async () => {
    let added = false;
    const warehouse = {
      name: "warehouse",
      transport: "network",
      availability: "connected",
      profile: null,
      connection: { type: "network", host: "10.0.5.20", port: 9100 },
    };
    renderPage(((input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/status") return Promise.resolve(json(status));
      if (url === "/api/profiles/list") return Promise.resolve(json({ profiles: [] }));
      if (url === "/api/printers/add") {
        added = true;
        return Promise.resolve(json({ name: "warehouse", transport: "network", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: added ? [warehouse] : [] }));
    }) as typeof globalThis.fetch);
    expect(await screen.findByText("No printers configured.")).toBeTruthy();

    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Add network printer manually" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    const [row, card] = await screen.findAllByText("warehouse");
    expect(row?.closest("tr")?.classList.contains("printer-row-found")).toBe(true);
    expect(card?.closest("article")?.classList.contains("printer-row-found")).toBe(true);
    expect(screen.queryByRole("heading", { name: "Add network printer" })).toBeNull();
  });

  test("a printer that has just become unavailable carries the lost highlight", async () => {
    jest.useFakeTimers();
    const availabilities = ["connected", "unavailable"];
    let poll = 0;
    renderPage(((input: RequestInfo | URL) => {
      if (String(input) !== "/api/printers/list") {
        return Promise.resolve(json(status));
      }
      const availability = availabilities[Math.min(poll, availabilities.length - 1)];
      poll += 1;
      return Promise.resolve(json({
        printers: [{
          name: "kitchen",
          transport: "network",
          availability,
          profile: null,
          connection: { type: "network", host: "10.42.0.71", port: 9100 },
        }],
      }));
    }) as typeof globalThis.fetch);
    await act(async () => { await advanceTimers(0); });
    expect(screen.getAllByText("kitchen")).toHaveLength(2);

    await act(async () => { await advanceTimers(10_000); });
    const [row, card] = screen.getAllByText("kitchen");
    expect(row?.closest("tr")?.classList.contains("printer-row-lost")).toBe(true);
    expect(card?.closest("article")?.classList.contains("printer-row-lost")).toBe(true);
  });
});
