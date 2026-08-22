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
// has to state the one it is actually about. The networks are among them
// now: the discovery card is always mounted and states its scope from
// them, so a stub that leaves them out breaks tests that are about
// something else entirely.
function shared(url: string) {
  if (url === "/api/status") return json(status);
  if (url === "/api/profiles/list") return json({ profiles: [] });
  if (url === "/api/printers/discover/networks") return json(networks);
  return null;
}

function fetchStub(printers: unknown = { printers: [] }) {
  return ((input: RequestInfo | URL) => {
    const common = shared(String(input));
    return Promise.resolve(common ?? json(printers));
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

function addManually() {
  fireEvent.click(screen.getByRole("button", { name: "Add IP printer manually" }));
}

function expandOptions() {
  fireEvent.click(screen.getByRole("button", { name: "Scan options" }));
}

// Whether two controls share the one bar under the accordion, and whether one
// control follows another in the document. Both answer with a boolean, so a
// failure prints a word instead of the entire page.
function sameBar(one: string, other: string) {
  const bar = (name: string) => screen.getByRole("button", { name }).closest("footer");
  return bar(one) !== null && bar(one) === bar(other);
}

function below(first: string, second: string) {
  return precedes(screen.getByRole("button", { name: first }), screen.getByRole("button", { name: second }));
}

function precedes(first: Element, second: Element) {
  return (first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
}

// The scope line under `Discovery`, read through the element the disclosure
// describes itself with — by id, because the options footer states some of
// the same phrases and this is about the one that stays visible.
function statedScope() {
  return document.getElementById("scan-options-scope")?.textContent;
}

// The section's one button scans with the scope the options state, and they
// can only state one once the machine's networks are known — so pressing it
// means waiting for it to stand for something first.
async function scan(name: "Scan" | "Rescan") {
  const button = await screen.findByRole("button", { name });
  await waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
  await act(async () => { fireEvent.click(button); });
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
    renderPage(((input: RequestInfo | URL) => {
      const common = shared(String(input));
      return common
        ? Promise.resolve(common)
        : new Promise<Response>((resolve) => { resolvePrinters = resolve; });
    }) as typeof globalThis.fetch);
    expect(screen.getByText("Loading printers…")).toBeTruthy();
    await act(async () => { resolvePrinters(json({ printers: [] })); });
    expect(await screen.findByText("No printers configured.")).toBeTruthy();

    cleanup();
    renderPage(((input: RequestInfo | URL) => Promise.resolve(
      shared(String(input))
      ?? json({ error: { code: "printer_inventory_unavailable", message: "Printer inventory is unavailable." } }, 500),
    )) as typeof globalThis.fetch);
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
    renderPage(((input: RequestInfo | URL) => Promise.resolve(
      shared(String(input)) ?? inventories.shift()!,
    )) as typeof globalThis.fetch);
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

  // Both layouts render the same printer, so both have to state the same
  // facts — this list has been a place where a change landed in one and not
  // the other. The transport is one of those facts: it moved into the
  // connection cell as a tag rather than leaving the page.
  test("renders matching desktop-table and mobile-card printer facts", async () => {
    const counter = {
      name: "counter",
      transport: "usb",
      availability: "connected",
      profile: null,
      connection: { type: "usb", vendor_id: 0x0416, product_id: 0x5011, bus: "003", address: 4, manufacturer: null, product: null, serial_number: "B120300001", interface_number: 0, out_endpoints: [1], in_endpoints: [] },
    };
    renderPage(fetchStub({ printers: [printer, counter] }));
    expect(await screen.findAllByText("Kitchen")).toHaveLength(2);
    expect(screen.getAllByText("Connected")).toHaveLength(4);
    expect(screen.getAllByText("REFERENCE")).toHaveLength(2);
    expect(screen.getAllByText("10.0.0.8:9100")).toHaveLength(2);

    // `IP` rather than `Network`: the word the interface uses for a printer
    // reached over the network, in both layouts. The wire still says
    // `network`, which is why the tag is a rendering and not a rename.
    expect(screen.getAllByText("IP")).toHaveLength(2);
    expect(screen.getAllByText("USB")).toHaveLength(2);
    expect(gone(screen.queryByRole("columnheader", { name: "Transport" }))).toBe(true);
    expect(screen.getAllByRole("columnheader").map((cell) => cell.textContent))
      .toEqual(["Name", "Status", "Profile", "Connection"]);

    // The tag is a word to a screen reader rather than two letters floating
    // in front of an address.
    expect(screen.getAllByText(/^connection$/).length).toBeGreaterThan(0);
  });

  // Two named blocks, both named after printers rather than one after an
  // activity, and every control that acts on discovery in one bar under its
  // accordion.
  test("the page reads as Printer Discovery and Configured Printers, with one bar of controls", async () => {
    renderPage(fetchStub());
    expect(await screen.findByText("USB · 2 networks · 507 probes")).toBeTruthy();

    expect(screen.getAllByRole("heading", { name: "Printer Discovery" })).toHaveLength(1);
    expect(screen.getAllByRole("heading", { name: "Configured Printers" })).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Scan options" }).getAttribute("aria-expanded")).toBe("false");

    // The bar is one row: Reset on the left, the two actions on the right —
    // and it is there with the form shut, because resetting the scope is
    // meaningful whether or not the fields are on screen.
    expect(sameBar("Reset", "Add IP printer manually")).toBe(true);
    expect(sameBar("Reset", "Scan")).toBe(true);
    // Below the accordion, not above it: the title row holds the title.
    expect(below("Scan options", "Scan")).toBe(true);

    // Nothing that looks like a menu is left, and the form has no second
    // button to disagree with the one in the bar.
    expect(gone(screen.queryByRole("button", { name: "Discovery options" }))).toBe(true);
    expect(gone(screen.queryByRole("menuitem"))).toBe(true);
    expect(gone(screen.queryByRole("button", { name: "Start scan" }))).toBe(true);
    expect(gone(screen.queryByRole("button", { name: "Refresh" }))).toBe(true);
  });

  // Options, then what happened, then what to do next. The button that starts
  // a scan sits after the results it produced rather than above them, and the
  // three are one card rather than two with a bar floating between.
  test("the discovery card reads options, then results, then controls", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());
    await scan("Scan");
    await act(async () => { stream().emit("printer", discovered()); });

    const options = screen.getByRole("button", { name: "Scan options" });
    const count = screen.getByText("1 printer found (1 new)");
    const add = screen.getByRole("button", { name: "Add 10.0.5.20:9100" });
    const reset = screen.getByRole("button", { name: "Reset" });

    expect(precedes(options, count)).toBe(true);
    expect(precedes(count, add)).toBe(true);
    expect(precedes(add, reset)).toBe(true);

    // The element the disclosure sits in is the card, and it holds the
    // results and the bar as well — one container, not a stack of them.
    const card = options.parentElement!;
    expect(card.contains(add)).toBe(true);
    expect(card.contains(reset)).toBe(true);
  });

  // One slot, three jobs: start, stop, repeat. A separate Cancel down in the
  // progress line would be a second place to look for the same scan.
  test("the one scan button becomes Cancel while scanning and Rescan afterwards", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());

    // Both transports and every network, which is the CLI's no-flag scope,
    // with the port and timeout the card asked the server for.
    await scan("Scan");
    expect(FakeEventSource.urls).toEqual(["/api/printers/discover?port=9100&timeout=1000"]);
    expect(gone(screen.queryByRole("button", { name: "Scan" }))).toBe(true);
    expect(gone(screen.queryByRole("button", { name: "Rescan" }))).toBe(true);

    await act(async () => { stream().emit("printer", discovered()); });
    expect(screen.getByRole("button", { name: "Add 10.0.5.20:9100" })).toBeTruthy();

    await act(async () => { stream().emit("completed", {}); });
    expect(screen.getByRole("button", { name: "Rescan" })).toBeTruthy();
    expect(gone(screen.queryByRole("button", { name: "Cancel" }))).toBe(true);
  });

  // Cancel stops the probing; it does not un-find what was already found. A
  // sweep interrupted after it reached a printer has produced something worth
  // keeping, and the browser — unlike a terminated `printers discover` — is
  // still on screen to keep it.
  // The scan line's USB clause is the one fact the stream cannot carry, so it
  // comes from the scope the scan started with. Not from a constant, and not
  // from the live checkbox: a reader who unticks USB while a scan runs has
  // changed the next scan, not the one being reported.
  test("the scan line reports the USB half of the scan that ran, not of the form", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());
    await act(async () => {});

    expandOptions();
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("USB Printers"));
    await scan("Scan");
    await act(async () => { stream().emit("progress", { completed: 12, total: 507 }); });

    expect(screen.getByText("Scanning 12 / 507 IP addresses")).toBeTruthy();

    // Reticking USB describes the next scan, and may not rewrite this one.
    expandOptions();
    fireEvent.click(screen.getByLabelText("USB Printers"));
    expect(statedScope()).toBe("USB · 2 networks · 507 probes");
    expect(screen.getByText("Scanning 12 / 507 IP addresses")).toBeTruthy();
  });

  test("Cancel stops the scan, keeps its results, and says where it stopped", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());

    await scan("Scan");
    await act(async () => {
      stream().emit("prepared", { targets: [], skipped: [], total_probes: 508 });
      stream().emit("printer", discovered());
      stream().emit("progress", { completed: 257, total: 508 });
    });
    expect(screen.getByText("Checking USB · scanning 257 / 508 IP addresses")).toBeTruthy();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Cancel" })); });

    // The row is still listed and still addable, the count still counts it,
    // and the line refuses to claim the sweep finished.
    expect(screen.getByRole("button", { name: "Add 10.0.5.20:9100" })).toBeTruthy();
    expect(screen.getByText("1 printer found (1 new)")).toBeTruthy();
    expect(screen.getByText("Checked USB · scanned 257 / 508 IP addresses")).toBeTruthy();
    expect(gone(screen.queryByRole("progressbar"))).toBe(true);
    expect(screen.getByRole("button", { name: "Rescan" })).toBeTruthy();
    expect(gone(screen.queryByRole("button", { name: "Cancel" }))).toBe(true);
  });

  test("scanning shuts the options, and the next scan reuses the settings it was configured with", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());
    await act(async () => {});

    expandOptions();
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9101" } });

    await scan("Scan");
    expect(FakeEventSource.urls).toEqual(["/api/printers/discover?port=9101&timeout=1000"]);
    expect(screen.getByRole("button", { name: "Scan options" }).getAttribute("aria-expanded")).toBe("false");

    // Stopping leaves a scan behind, so the slot reads `Rescan` from here on.
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Cancel" })); });
    await scan("Rescan");
    expect(FakeEventSource.urls[1]).toBe("/api/printers/discover?port=9101&timeout=1000");
  });

  // The line under `Discovery` is the only place the page says what a scan
  // will cover and cost, so the button may not scan something else —
  // including a change made in the options and never started.
  test("the scan button sends the scope the options state, and is refused when they state none", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());
    await act(async () => {});

    expandOptions();
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    expect(statedScope()).toBe("USB · 1 of 2 networks · 253 probes");

    await scan("Scan");
    expect(FakeEventSource.urls).toEqual(["/api/printers/discover?subnet=10.42.0.0%2F24&port=9100&timeout=1000"]);

    // Nothing left to scan leaves nothing to press: the only button that
    // starts a scan is refused exactly when the options name no scan.
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Cancel" })); });
    expandOptions();
    fireEvent.click(screen.getByLabelText("Network (IP) Printers"));
    fireEvent.click(screen.getByLabelText("USB Printers"));
    expect(statedScope()).toBe("Nothing to scan");
    expect(screen.getByRole("button", { name: "Rescan" }).hasAttribute("disabled")).toBe(true);
  });

  // The line and the button are drawn by one render from one value, so the
  // moment the line changes the button has already changed with it.
  //
  // Raw DOM events rather than `fireEvent`, and microtasks rather than `act`,
  // because `act` flushes renders and effects together — a coincidence no
  // browser guarantees. Preact commits the DOM on a microtask and runs
  // effects after paint, so between those two moments a button fed by an
  // effect still carries the scope from before the change the line is already
  // showing. That gap is how a sweep of 1,265 addresses went out under a line
  // reading 254.
  test("a scan started the moment the line changes sends the scope the line shows", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());
    await act(async () => {});

    expandOptions();
    await screen.findByLabelText("10.42.0.0/24");

    (screen.getByLabelText("192.168.1.0/24") as HTMLInputElement).click();
    await Promise.resolve();
    await Promise.resolve();
    // The line has narrowed; effects have not run yet.
    expect(statedScope()).toBe("USB · 1 of 2 networks · 253 probes");

    (screen.getByRole("button", { name: "Scan" }) as HTMLButtonElement).click();
    await act(async () => {});

    expect(FakeEventSource.urls).toEqual(["/api/printers/discover?subnet=10.42.0.0%2F24&port=9100&timeout=1000"]);
  });

  // A dead button with no visible reason is the failure this replaces: the
  // options open themselves so the refusal, its cause and its remedy arrive
  // together.
  test("a failed network detection refuses the scan and opens its own reason", async () => {
    renderPage(((input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/printers/discover/networks") {
        return Promise.resolve(json({ error: { code: "network_detection_failed", message: "Unable to detect this machine's networks." } }, 500));
      }
      return Promise.resolve(shared(url) ?? json({ printers: [] }));
    }) as typeof globalThis.fetch);

    expect(await screen.findByText("Unable to detect this machine's networks.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Scan options" }).getAttribute("aria-expanded")).toBe("true");
    expect(statedScope()).toBe("USB · networks unavailable");
    expect(screen.getByRole("button", { name: "Scan" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
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
      const common = shared(url);
      if (common) return Promise.resolve(common);
      if (url === "/api/printers/add") {
        added = true;
        return Promise.resolve(json({ name: "warehouse", transport: "network", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: added ? [warehouse] : [] }));
    }) as typeof globalThis.fetch);
    expect(await screen.findByText("No printers configured.")).toBeTruthy();

    addManually();
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    await expectFlash("warehouse", "printer-row-found");
    expect(gone(screen.queryByRole("heading", { name: "Add IP printer" }))).toBe(true);
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
      const common = shared(url);
      if (common) return Promise.resolve(common);
      if (url === "/api/printers/add") {
        added = true;
        return Promise.resolve(json({ name: "warehouse", transport: "network", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: added ? [warehouse] : [] }));
    }) as typeof globalThis.fetch);
    await act(async () => {});

    await scan("Scan");
    await act(async () => { stream().emit("printer", discovered()); });
    expect(screen.getByText("1 printer found (1 new)")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Add 10.0.5.20:9100" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    // Recorded while the sweep is still running, which is when adding
    // actually happens.
    expect(await screen.findByText("1 printer found (0 new)")).toBeTruthy();
    expect(gone(screen.queryByRole("button", { name: "Add 10.0.5.20:9100" }))).toBe(true);
    await expectFlash("warehouse", "printer-row-found");
  });

  test("a registered result stays out of the results across a route change", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(((input: RequestInfo | URL) => {
      const url = String(input);
      const common = shared(url);
      if (common) return Promise.resolve(common);
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

    await scan("Scan");
    await act(async () => { stream().emit("printer", discovered()); });
    fireEvent.click(screen.getByRole("button", { name: "Add 10.0.5.20:9100" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });

    expect(gone(screen.queryByRole("button", { name: "Add 10.0.5.20:9100" }))).toBe(true);
    expect(screen.getByText("1 printer found (0 new)")).toBeTruthy();
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

    expandOptions();
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    await scan("Scan");
    const narrowed = "/api/printers/discover?subnet=10.42.0.0%2F24&port=9100&timeout=1000";
    expect(FakeEventSource.urls).toEqual([narrowed]);
    // Finished before the detour, so the slot holds `Rescan` rather than the
    // `Cancel` a running scan puts there.
    await act(async () => { stream().emit("completed", {}); });

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });

    // The line the page comes back showing is the narrowed scope, and
    // `Rescan` sends exactly it.
    expect(await screen.findByText("USB · 1 of 2 networks · 253 probes")).toBeTruthy();
    await scan("Rescan");
    expect(FakeEventSource.urls[1]).toBe(narrowed);
  });

  test("the scan options reopen showing the scope the last scan ran with", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(fetchStub());
    await act(async () => {});

    expandOptions();
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    fireEvent.input(screen.getByLabelText("RAW TCP port"), { target: { value: "9101" } });
    await scan("Scan");

    // The form is where a reader goes to answer "what will Rescan do?", so
    // it may not answer with the default when the last scan was narrowed.
    expandOptions();
    await screen.findByLabelText("10.42.0.0/24");
    expect(checkbox("10.42.0.0/24").checked).toBe(true);
    expect(checkbox("192.168.1.0/24").checked).toBe(false);
    expect((screen.getByLabelText("RAW TCP port") as HTMLInputElement).value).toBe("9101");
    expect(statedScope()).toBe("USB · 1 of 2 networks · 253 probes");
  });

  // The card detects the networks once per mount, so this is the case a
  // route change produces: the page comes back, asks again, and the subnet
  // the last scan ran on is gone.
  test("a network that has since disappeared comes back in the custom field", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    let detections = 0;
    globalThis.fetch = ((input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/printers/discover/networks") {
        detections += 1;
        // The cable came out while the reader was on another route, so the
        // subnet the scan ran on is no longer one of this machine's adapters.
        return Promise.resolve(json(detections === 1
          ? networks
          : { ...networks, networks: networks.networks.slice(1) }));
      }
      return Promise.resolve(shared(url) ?? json({ printers: [] }));
    }) as typeof globalThis.fetch;
    render(<AppDataProvider><Navigable /></AppDataProvider>);
    await act(async () => {});

    expandOptions();
    await screen.findByLabelText("10.42.0.0/24");
    fireEvent.click(screen.getByLabelText("192.168.1.0/24"));
    await scan("Scan");

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Leave the printers page" })); });

    expandOptions();
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
      const common = shared(url);
      if (common) return Promise.resolve(common);
      if (url === "/api/printers/add") {
        return Promise.resolve(json({ name: "warehouse", transport: "network", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    await act(async () => {});

    await scan("Scan");
    await act(async () => {
      stream().emit("printer", discovered());
      stream().emit("printer", discovered({ connection: { type: "network", host: "10.0.5.21", port: 9100 } }));
    });

    addManually();
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "warehouse" } });
    fireEvent.input(screen.getByLabelText("Host"), { target: { value: "10.0.5.20" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    // The host typed here is the one the scan is listing, and nothing but
    // this dialog ever knew it.
    expect(await screen.findByText("2 printers found (1 new)")).toBeTruthy();
    expect(gone(screen.queryByRole("button", { name: "Add 10.0.5.20:9100" }))).toBe(true);
    // The neighbour on the next address is a different printer and is still
    // offered: registering one endpoint may never claim another.
    expect(screen.getByRole("button", { name: "Add 10.0.5.21:9100" })).toBeTruthy();
  });

  test("registering a discovered USB printer moves it out of the results too", async () => {
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderPage(((input: RequestInfo | URL) => {
      const url = String(input);
      const common = shared(url);
      if (common) return Promise.resolve(common);
      if (url === "/api/printers/add") {
        return Promise.resolve(json({ name: "counter", transport: "usb", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    await act(async () => {});

    await scan("Scan");
    await act(async () => { stream().emit("printer", discoveredUsb()); });

    fireEvent.click(screen.getByRole("button", { name: "Add POS-58 Printer" }));
    fireEvent.input(screen.getByLabelText("Name"), { target: { value: "counter" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Add printer" })); });

    expect(await screen.findByText("1 printer found (0 new)")).toBeTruthy();
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
      const common = shared(url);
      if (common) return Promise.resolve(common);
      if (url === "/api/printers/add") {
        return Promise.resolve(json({ name: "counter", transport: "usb", profile: null, warnings: [] }));
      }
      return Promise.resolve(json({ printers: [] }));
    }) as typeof globalThis.fetch);
    await act(async () => {});

    await scan("Scan");
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

    expect(await screen.findByText("2 printers found (1 new)")).toBeTruthy();
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

    await scan("Scan");
    await act(async () => {
      stream().emit("printer", discovered({
        configured_names: ["kitchen"],
        connection: { type: "network", host: "10.42.0.71", port: 9100 },
      }));
    });

    // Counted, never listed, and the printer it already is lights up — proved
    // reachable by the scan rather than by waiting for the next poll.
    expect(gone(screen.queryByRole("button", { name: "Add 10.42.0.71:9100" }))).toBe(true);
    expect(screen.getByText("1 printer found (0 new)")).toBeTruthy();
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
      const common = shared(url);
      if (common) return Promise.resolve(common);
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

    addManually();
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

  test("a printer that has just become unavailable carries the lost highlight", async () => {
    jest.useFakeTimers();
    const availabilities = ["connected", "unavailable"];
    let poll = 0;
    renderPage(((input: RequestInfo | URL) => {
      const common = shared(String(input));
      if (common) {
        return Promise.resolve(common);
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
