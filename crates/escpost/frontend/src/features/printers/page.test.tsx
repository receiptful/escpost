import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { useState } from "preact/hooks";
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
  // Two, so that a selection can be partial: one checked network out of two
  // is an explicit subnet on the wire, where every network checked is
  // automatic mode and names none.
  networks: [
    { subnet: "10.42.0.0/24", interface: "enx0", hosts: 253 },
    { subnet: "192.168.1.0/24", interface: "wlp3s0", hosts: 254 },
  ],
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
// `data.test.tsx` uses. It dispatches: a results panel with nothing in it
// cannot show what happens to a row, so every test about registering a
// discovered printer needs the stream to actually deliver one.
class FakeEventSource {
  static instances: FakeEventSource[] = [];
  static urls: string[] = [];
  private readonly listeners = new Map<string, ((event: Event) => void)[]>();

  constructor(url: string) {
    FakeEventSource.urls.push(url);
    FakeEventSource.instances.push(this);
  }

  addEventListener(name: string, handler: (event: Event) => void) {
    const existing = this.listeners.get(name) ?? [];
    existing.push(handler);
    this.listeners.set(name, existing);
  }

  emit(name: string, payload: unknown) {
    for (const handler of this.listeners.get(name) ?? []) {
      handler(new MessageEvent(name, { data: JSON.stringify(payload) }));
    }
  }

  close() {}
}

// The stream the page most recently opened.
function stream() {
  return FakeEventSource.instances[FakeEventSource.instances.length - 1]!;
}

function discovered(overrides: Record<string, unknown> = {}) {
  return {
    transport: "network",
    configured_names: [],
    configured_profile: null,
    interface: "enx0",
    connection: { type: "network", host: "10.0.5.20", port: 9100 },
    ...overrides,
  };
}

// A serial-less device by default, which is the case where two of them are
// indistinguishable by anything but where they are plugged in.
function discoveredUsb(connection: Record<string, unknown> = {}) {
  return {
    transport: "usb",
    configured_names: [],
    configured_profile: null,
    connection: {
      type: "usb",
      vendor_id: 0x0416,
      product_id: 0x5011,
      bus: "003",
      address: 7,
      manufacturer: null,
      product: "POS-58 Printer",
      serial_number: null,
      interface_number: 0,
      out_endpoints: [0x01],
      in_endpoints: [],
      ...connection,
    },
  };
}

const originalEventSource = globalThis.EventSource;

function renderPage(fetch: typeof globalThis.fetch) {
  globalThis.fetch = fetch;
  return render(<AppDataProvider><PrintersPage /></AppDataProvider>);
}

// Stands in for the router, which unmounts a route component on navigation
// while the provider above it stays mounted.
function Navigable() {
  const [onPrinters, setOnPrinters] = useState(true);
  return (
    <>
      <button type="button" onClick={() => setOnPrinters((current) => !current)}>Leave the printers page</button>
      {onPrinters && <PrintersPage />}
    </>
  );
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

// Absence as a boolean. `expect(node).toBeNull()` prints the entire happy-dom
// node graph when it fails — tens of megabytes for a node still attached to
// the page — which buries every other failure in the run.
function gone(element: Element | null) {
  return element === null;
}

// Waits for a registered printer's highlight on both layouts. The flash is
// raised only once the forced refresh has resolved — later than the row it
// belongs to, deliberately, so that a slow poll cannot let the window expire
// before there is anything to highlight.
async function expectFlash(name: string, flash: string) {
  await waitFor(() => {
    const [row, card] = screen.getAllByText(name);
    expect(row?.closest("tr")?.classList.contains(flash)).toBe(true);
    expect(card?.closest("article")?.classList.contains(flash)).toBe(true);
  });
}

function checkbox(label: string) {
  return screen.getByLabelText(label) as HTMLInputElement;
}

function openMenu() {
  fireEvent.click(screen.getByRole("button", { name: "Discovery options" }));
}

afterEach(() => {
  cleanup();
  jest.useRealTimers();
  globalThis.EventSource = originalEventSource;
  FakeEventSource.urls = [];
  FakeEventSource.instances = [];
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

    expect(gone(screen.queryByRole("button", { name: "Refresh" }))).toBe(true);
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
    expect(gone(screen.queryByRole("menuitem", { name: "Scan options…" }))).toBe(true);

    openMenu();
    expect(screen.getByRole("menuitem", { name: "Scan options…" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "Add network printer manually" })).toBeTruthy();
    expect(gone(screen.queryByRole("button", { name: "Refresh" }))).toBe(true);
  });

  test("Discover printers scans with the current settings, and Cancel discards the scan", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());
    await act(async () => {});

    // No port and no timeout: nobody has chosen either, and the endpoint owns
    // both defaults. The page never states a number it does not own.
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Discover printers" })); });
    expect(FakeEventSource.urls).toEqual(["/api/printers/discover"]);
    expect(screen.getByRole("heading", { name: "Discovering printers" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Rescan" })).toBeTruthy();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Cancel" })); });
    expect(gone(screen.queryByRole("heading", { name: "Discovering printers" }))).toBe(true);
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
    expect(gone(screen.queryByRole("heading", { name: "Scan options" }))).toBe(true);

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

    await expectFlash("warehouse", "printer-row-found");
    expect(gone(screen.queryByRole("heading", { name: "Add network printer" }))).toBe(true);
  });

  test("registering a discovered printer moves it out of the results and into the count", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
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
    await act(async () => {});

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Discover printers" })); });
    await act(async () => { stream().emit("printer", discovered()); });
    expect(screen.getByText("1 new so far")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Add 10.0.5.20:9100" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    // Recorded while the sweep is still running, which is when adding
    // actually happens.
    expect(await screen.findByText("0 new so far · 1 already configured")).toBeTruthy();
    expect(gone(screen.queryByRole("button", { name: "Add 10.0.5.20:9100" }))).toBe(true);
    await expectFlash("warehouse", "printer-row-found");
  });

  test("a registered result stays out of the results across a route change", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(((input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/status") return Promise.resolve(json(status));
      if (url === "/api/profiles/list") return Promise.resolve(json({ profiles: [] }));
      if (url === "/api/printers/add") {
        return Promise.resolve(json({ name: "warehouse", transport: "network", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    cleanup();
    // The scan outlives the page on purpose, so anything the page learned
    // about the scan has to outlive it too — a route component is unmounted
    // by the router, and coming back must not offer a printer that has just
    // been registered.
    render(<AppDataProvider><Navigable /></AppDataProvider>);
    await act(async () => {});

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Discover printers" })); });
    await act(async () => { stream().emit("printer", discovered()); });
    fireEvent.click(screen.getByRole("button", { name: "Add 10.0.5.20:9100" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });

    expect(gone(screen.queryByRole("button", { name: "Add 10.0.5.20:9100" }))).toBe(true);
    expect(screen.getByText("0 new so far · 1 already configured")).toBeTruthy();
  });

  test("the chosen scan scope survives a route change, so Rescan repeats the same sweep", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    globalThis.fetch = fetchStub();
    // The scope a scan ran with belongs to the scan, and the scan outlives
    // the page: narrowing a sweep to one segment and then walking to another
    // route may not quietly widen it back to every network this machine is
    // on, which is what `Rescan` would then send.
    render(<AppDataProvider><Navigable /></AppDataProvider>);
    await act(async () => {});

    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Scan options…" }));
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Start scan" })); });
    const narrowed = "/api/printers/discover?subnet=10.42.0.0%2F24&port=9100&timeout=1000";
    expect(FakeEventSource.urls).toEqual([narrowed]);

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Rescan" })); });
    expect(FakeEventSource.urls[1]).toBe(narrowed);
  });

  test("the scan options panel reopens showing the scope the last scan ran with", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    globalThis.fetch = fetchStub();
    renderPage(fetchStub());
    await act(async () => {});

    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Scan options…" }));
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9101" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Start scan" })); });

    // The panel is where a reader goes to answer "what will Rescan do?", so
    // it may not answer with the default when the last scan was narrowed.
    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Scan options…" }));
    await screen.findByLabelText("10.42.0.0/24");
    expect(checkbox("10.42.0.0/24").checked).toBe(true);
    expect(checkbox("192.168.1.0/24").checked).toBe(false);
    expect((screen.getByLabelText("RAW TCP port") as HTMLInputElement).value).toBe("9101");
    expect(screen.getByText("253 probes")).toBeTruthy();
  });

  test("a network that has since disappeared reopens in the custom field", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    let detections = 0;
    renderPage(((input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/status") return Promise.resolve(json(status));
      if (url === "/api/profiles/list") return Promise.resolve(json({ profiles: [] }));
      if (url === "/api/printers/discover/networks") {
        detections += 1;
        // The cable came out between the two openings, so the subnet the
        // scan ran on is no longer one of this machine's adapters.
        return Promise.resolve(json(detections === 1
          ? networks
          : { ...networks, networks: networks.networks.slice(1) }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    await act(async () => {});

    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Scan options…" }));
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Start scan" })); });

    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Scan options…" }));
    await screen.findByLabelText("192.168.1.0/24");
    // A chosen subnet with no adapter behind it has no row to be checked in,
    // so it lands where the reader would have to retype it rather than
    // vanishing from the selection.
    expect(gone(screen.queryByLabelText("10.42.0.0/24"))).toBe(true);
    expect((screen.getByLabelText("Custom network") as HTMLInputElement).value).toBe("10.42.0.0/24");
  });

  test("a printer registered manually stops being offered by the running scan", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(((input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/status") return Promise.resolve(json(status));
      if (url === "/api/profiles/list") return Promise.resolve(json({ profiles: [] }));
      if (url === "/api/printers/add") {
        return Promise.resolve(json({ name: "warehouse", transport: "network", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    await act(async () => {});

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Discover printers" })); });
    await act(async () => {
      stream().emit("printer", discovered());
      stream().emit("printer", discovered({ connection: { type: "network", host: "10.0.5.21", port: 9100 } }));
    });

    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Add network printer manually" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    // The host typed here is the one the scan is listing, and nothing but
    // this dialog ever knew it.
    expect(await screen.findByText("1 new so far · 1 already configured")).toBeTruthy();
    expect(gone(screen.queryByRole("button", { name: "Add 10.0.5.20:9100" }))).toBe(true);
    // The neighbour on the next address is a different printer and is still
    // offered: registering one endpoint may never claim another.
    expect(screen.getByRole("button", { name: "Add 10.0.5.21:9100" })).toBeTruthy();
  });

  test("registering a discovered USB printer moves it out of the results too", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(((input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/status") return Promise.resolve(json(status));
      if (url === "/api/profiles/list") return Promise.resolve(json({ profiles: [] }));
      if (url === "/api/printers/add") {
        return Promise.resolve(json({ name: "counter", transport: "usb", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    await act(async () => {});

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Discover printers" })); });
    await act(async () => { stream().emit("printer", discoveredUsb()); });

    fireEvent.click(screen.getByRole("button", { name: "Add POS-58 Printer" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "counter" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    expect(await screen.findByText("0 new so far · 1 already configured")).toBeTruthy();
    expect(gone(screen.queryByRole("button", { name: "Add POS-58 Printer" }))).toBe(true);
  });

  // `classify_usb_printers` gives one saved printer at most one connected
  // interface, so the terminal keeps offering the second of two identical
  // devices after the first is registered. Marking every match instead would
  // hide a printer nobody registered, with no way back but a rescan.
  test("registering one of two indistinguishable USB printers leaves the other offered", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(((input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/status") return Promise.resolve(json(status));
      if (url === "/api/profiles/list") return Promise.resolve(json({ profiles: [] }));
      if (url === "/api/printers/add") {
        return Promise.resolve(json({ name: "counter", transport: "usb", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    await act(async () => {});

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Discover printers" })); });
    await act(async () => {
      stream().emit("printer", discoveredUsb());
      // Same vendor, product, interface and (absent) serial: only the port it
      // is plugged into tells the two apart.
      stream().emit("printer", discoveredUsb({ address: 8 }));
    });
    expect(screen.getAllByRole("button", { name: "Add POS-58 Printer" })).toHaveLength(2);

    fireEvent.click(screen.getAllByRole("button", { name: "Add POS-58 Printer" })[0]!);
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "counter" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    expect(await screen.findByText("1 new so far · 1 already configured")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Add POS-58 Printer" })).toHaveLength(1);
  });

  test("a scan re-finding a configured printer counts it and flashes its existing row", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub({
      printers: [{
        name: "kitchen",
        transport: "network",
        availability: "unavailable",
        profile: null,
        connection: { type: "network", host: "10.42.0.71", port: 9100 },
      }],
    }));
    expect(await screen.findAllByText("kitchen")).toHaveLength(2);

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Discover printers" })); });
    await act(async () => {
      stream().emit("printer", discovered({
        configured_names: ["kitchen"],
        connection: { type: "network", host: "10.42.0.71", port: 9100 },
      }));
    });

    // Counted, never listed, and the printer it already is lights up — proved
    // reachable by the scan rather than by waiting for the next poll.
    expect(gone(screen.queryByRole("button", { name: "Add 10.42.0.71:9100" }))).toBe(true);
    expect(screen.getByText("0 new so far · 1 already configured")).toBeTruthy();
    const [row, card] = screen.getAllByText("kitchen");
    expect(row?.closest("tr")?.classList.contains("printer-row-found")).toBe(true);
    expect(card?.closest("article")?.classList.contains("printer-row-found")).toBe(true);
    expect(screen.getAllByText("Connected")).toHaveLength(2);
  });

  test("an add that lands while an inventory poll is in flight still reaches the inventory", async () => {
    let listCalls = 0;
    let added = false;
    let releaseFirstPoll!: (response: Response) => void;
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
      listCalls += 1;
      // The first poll is still open when the printer is saved, so its
      // response cannot possibly contain it.
      if (listCalls === 1) {
        return new Promise<Response>((resolve) => { releaseFirstPoll = resolve; });
      }
      return Promise.resolve(json({ printers: added ? [warehouse] : [] }));
    }) as typeof globalThis.fetch);
    await act(async () => {});

    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Add network printer manually" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });
    expect(listCalls).toBe(1);

    await act(async () => { releaseFirstPoll(json({ printers: [] })); });
    // The released response is the one that predates the printer, and it is
    // empty; the printer can only appear through a request issued after it.
    await expectFlash("warehouse", "printer-row-found");
    expect(listCalls).toBe(2);
  });

  test("the discovery menu holds focus, moves it with the arrow keys, and hands it back on Escape", async () => {
    renderPage(fetchStub());
    await act(async () => {});

    openMenu();
    const items = screen.getAllByRole("menuitem");
    // By index rather than by node, so a failure reports a number instead of
    // printing an entire DOM element.
    const focused = () => items.indexOf(document.activeElement as HTMLButtonElement);
    expect(focused()).toBe(0);

    fireEvent.keyDown(document, { key: "ArrowDown" });
    expect(focused()).toBe(1);
    fireEvent.keyDown(document, { key: "ArrowDown" });
    expect(focused()).toBe(0);
    fireEvent.keyDown(document, { key: "ArrowUp" });
    expect(focused()).toBe(1);

    fireEvent.keyDown(document, { key: "Escape" });
    expect(gone(screen.queryByRole("menuitem"))).toBe(true);
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Discovery options");
  });

  test("a pointer press outside the menu closes it, and opening it closes the scan options", async () => {
    renderPage(fetchStub());
    await act(async () => {});

    openMenu();
    fireEvent.click(screen.getByRole("menuitem", { name: "Scan options…" }));
    expect(await screen.findByRole("heading", { name: "Scan options" })).toBeTruthy();

    // The menu and the panel are anchored to the same corner, so the menu may
    // never open behind the panel.
    openMenu();
    expect(gone(screen.queryByRole("heading", { name: "Scan options" }))).toBe(true);

    fireEvent.pointerDown(document.body);
    expect(gone(screen.queryByRole("menuitem"))).toBe(true);
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
