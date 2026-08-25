import { afterEach, beforeEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/preact";
import { LocationProvider } from "preact-iso";
import { locationStub } from "preact-iso/prerender";
import { App } from "../app";
import type { DiscoveryQuery } from "../api/discovery-stream";
import type { VirtualPrinterStatus } from "../api/types";
import { AppDataProvider, useAppData } from "./data";
import { ServerStatusProvider } from "./server-status-data";
import { AppShell } from "./shell";

const originalFetch = globalThis.fetch;
const originalEventSource = globalThis.EventSource;

let virtualPrinter: VirtualPrinterStatus | null = null;

beforeEach(() => {
  virtualPrinter = null;
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  globalThis.fetch = ((input: RequestInfo | URL) => {
    const path = String(input);
    if (path === "/api/status") {
      return Promise.reject(new Error("unexpected request to /api/status"));
    }
    const body = path === "/api/jobs/current"
      ? { receiving: false, profile: "REFERENCE", error: null, job: null }
      : path === "/api/profiles/list"
        ? { profiles: [] }
        : { printers: [] };
    return Promise.resolve(new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } }));
  }) as unknown as typeof globalThis.fetch;
});

afterEach(() => {
  cleanup();
  jest.useRealTimers();
  globalThis.fetch = originalFetch;
  globalThis.EventSource = originalEventSource;
});

const receiving: VirtualPrinterStatus = { state: "receiving", address: "127.0.0.1:9100" };
const ready: VirtualPrinterStatus = { state: "ready", address: "127.0.0.1:9100" };

// Drains the status request's promise chain (fetch -> json -> the provider's
// `.then` -> `.finally`), which takes more microtask turns than a single
// await gives it.
async function flush() {
  for (let i = 0; i < 6; i += 1) {
    await Promise.resolve();
  }
}

function renderAt(path: string) {
  locationStub(path);
  const view = render(<App />);
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("status", {
    virtual_printer: virtualPrinter,
    jobs_processed: 0,
    config_path: "/tmp/printers.toml",
  }));
  return view;
}

// Neither Bun's runtime nor the happy-dom registrator provides a global
// `EventSource`, so the shell's scan progress is driven through the same
// kind of stand-in `discovery-stream.test.ts` uses: it records listeners and
// lets a test dispatch a named event with a JSON payload.
class FakeEventSource {
  static instances: FakeEventSource[] = [];
  private readonly listeners = new Map<string, ((event: Event) => void)[]>();

  constructor(readonly url: string) {
    FakeEventSource.instances.push(this);
  }

  addEventListener(name: string, handler: (event: Event) => void) {
    this.listeners.set(name, [...(this.listeners.get(name) ?? []), handler]);
  }

  close() {}

  static forUrl(url: string) {
    return [...FakeEventSource.instances].reverse().find((source) => source.url === url);
  }

  static forUrlPrefix(prefix: string) {
    return [...FakeEventSource.instances].reverse().find((source) => source.url.startsWith(prefix));
  }

  emit(name: string, data: unknown) {
    const event = new MessageEvent(name, { data: JSON.stringify(data) });
    for (const handler of this.listeners.get(name) ?? []) {
      handler(event);
    }
  }
}

const scanQuery: DiscoveryQuery = { usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 };

// The printers page does not start scans yet, so the shell is rendered
// around a child that reaches for `startScan` the way that page eventually
// will.
function ScanProbe() {
  const { startScan } = useAppData();
  return <button type="button" onClick={() => startScan(scanQuery)}>Scan</button>;
}

async function renderShell(path: string) {
  locationStub(path);
  const view = render(
    <ServerStatusProvider>
      <AppDataProvider>
        <LocationProvider scope="/app">
          <AppShell><ScanProbe /></AppShell>
        </LocationProvider>
      </AppDataProvider>
    </ServerStatusProvider>,
  );
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("status", {
    virtual_printer: virtualPrinter,
    jobs_processed: 0,
    config_path: "/tmp/printers.toml",
  }));
  await act(async () => { await flush(); });
  return view;
}

async function startScanInShell(path: string) {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  const view = await renderShell(path);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });
  return { view, stream: FakeEventSource.forUrlPrefix("/api/printers/discover")! };
}

// Both responsive variants render the progress block, and every assertion
// about it is made against both: returning the pair (and failing here when
// either one is missing) keeps a variant from silently disappearing behind a
// test that only ever looked at the other.
function discoveryRegions() {
  return responsivePair("Printer discovery");
}

// The same pairing rule for the incoming-job block, which is rendered by the
// same component in the same two places.
function jobRegions() {
  return responsivePair("Print job");
}

function responsivePair(name: string) {
  const regions = screen.getAllByRole("region", { name });
  const sidebar = regions.find((region) => region.closest("aside"));
  const compact = regions.find((region) => region.closest("header"));
  expect(regions).toHaveLength(2);
  expect(sidebar).toBeTruthy();
  expect(compact).toBeTruthy();
  return [sidebar!, compact!];
}

function announcers(container: Element) {
  return [...container.querySelectorAll('[aria-live="polite"].sr-only')];
}

describe("App", () => {
  test("shows the current job workbench from the Print jobs route", async () => {
    renderAt("/app/jobs");

    expect(screen.getByRole("heading", { name: "Print jobs" }).getAttribute("class")).toContain("sr-only");
    expect(await screen.findByText("Waiting for first job")).toBeTruthy();
    expect(screen.queryByRole("link", { name: "Open current job viewer" })).toBeNull();
    expect(
      within(screen.getByRole("navigation", { name: "Workbench navigation" }))
        .getByRole("link", { name: "Print jobs" })
        .getAttribute("aria-current"),
    ).toBe("page");
  });

  test("keeps calibration honest while it is unavailable", () => {
    renderAt("/app/calibration");

    expect(screen.getByRole("heading", { name: "Calibration" })).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  test("shows a not found page for an unknown workbench route", () => {
    renderAt("/app/unknown");

    expect(screen.getByRole("heading", { name: "Not found" })).toBeTruthy();
  });

  test("exposes five destinations in each responsive navigation landmark", () => {
    renderAt("/app/jobs");

    expect(
      within(screen.getByRole("navigation", { name: "Workbench navigation" })).getAllByRole(
        "link",
      ),
    ).toHaveLength(5);
    expect(
      within(screen.getByRole("navigation", { name: "Mobile workbench navigation" })).getAllByRole(
        "link",
      ),
    ).toHaveLength(5);
  });

  test("exposes polite live server status semantics for both responsive variants", async () => {
    renderAt("/app/jobs");

    await screen.findAllByText("Ready");
    const statuses = screen.getAllByRole("status", { name: "Server status" });
    expect(statuses).toHaveLength(2);
    for (const status of statuses) {
      expect(status.getAttribute("aria-live")).toBe("polite");
      expect(status.getAttribute("aria-atomic")).toBe("true");
      expect(status.textContent).toContain("Ready");
    }
    const desktopStatus = statuses.find((status) => status.closest("aside"));
    expect(desktopStatus?.closest("aside")?.getAttribute("class")).toContain("hidden");
    expect(desktopStatus?.closest("aside")?.getAttribute("class")).toContain("lg:flex");
  });

  test("keeps the mobile server status in normal flow above content while only navigation is fixed", () => {
    const view = renderAt("/app/printers");

    const statuses = screen.getAllByRole("status", { name: "Server status" });
    const mobileStatus = statuses.find((status) => status.closest("header"));
    expect(mobileStatus?.closest("header")?.getAttribute("class")).toContain("lg:hidden");
    expect(mobileStatus?.closest("header")?.nextElementSibling?.tagName).toBe("MAIN");
    const fixedMobileBar = view.container.querySelector("div.fixed");
    expect(fixedMobileBar?.querySelector("header")).toBeNull();
    expect(
      fixedMobileBar?.querySelector('[aria-label="Mobile workbench navigation"]'),
    ).toBeTruthy();
  });

  test("selects Overview at the normalized workbench root path", () => {
    renderAt("/app/");

    expect(
      within(screen.getByRole("navigation", { name: "Workbench navigation" }))
        .getByRole("link", { name: "Overview" })
        .getAttribute("aria-current"),
    ).toBe("page");
  });

  test("visually marks the current destination in desktop and mobile navigation", () => {
    renderAt("/app/jobs");

    const desktop = within(screen.getByRole("navigation", { name: "Workbench navigation" }))
      .getByRole("link", { name: "Print jobs" });
    const mobile = within(screen.getByRole("navigation", { name: "Mobile workbench navigation" }))
      .getByRole("link", { name: "Print jobs" });
    expect(desktop.getAttribute("class")).toContain("menu-active");
    expect(mobile.getAttribute("class")).toContain("bg-primary");
  });

  test("lets pages own their width instead of centering the entire application", () => {
    const view = renderAt("/app/jobs");
    const pageContainer = view.container.querySelector("main > div");

    expect(pageContainer?.getAttribute("class")).toBe("flex w-full flex-col");
  });

  test("keeps semantic page headings while hiding every repeated visual title", () => {
    const pages = [
      ["/app/", "Overview"],
      ["/app/jobs", "Print jobs"],
      ["/app/printers", "Printers"],
      ["/app/profiles", "Profiles"],
      ["/app/calibration", "Calibration"],
    ];

    for (const [path, name] of pages) {
      const view = renderAt(path);
      expect(screen.getByRole("heading", { name }).getAttribute("class")).toContain("sr-only");
      view.unmount();
    }
  });

  test("replaces the old construction screen", () => {
    renderAt("/app/");

    expect(
      screen.queryByText("The new web workbench is under construction."),
    ).toBeNull();
  });

  test("a running scan shows progress in both responsive status variants on any page", async () => {
    const { stream } = await startScanInShell("/app/profiles");
    act(() => { stream.emit("progress", { completed: 312, total: 508 }); });

    for (const region of discoveryRegions()) {
      expect(within(region).getByText("Scanning printers")).toBeTruthy();
      expect(within(region).getByText("312 / 508")).toBeTruthy();
      expect(within(region).getByRole("link", { name: "View" }).getAttribute("href")).toBe("/app/printers");
      expect(region.querySelector("progress")?.getAttribute("value")).toBe("312");
    }
  });

  test("renders the sidebar variant as a card and the compact variant as an inline pill", async () => {
    const { stream } = await startScanInShell("/app/profiles");
    act(() => { stream.emit("progress", { completed: 312, total: 508 }); });

    const [sidebar, compact] = discoveryRegions();
    expect(sidebar.getAttribute("class")).toContain("text-sm");
    expect(compact.getAttribute("class")).toContain("text-xs");
    expect(compact.getAttribute("class")).toContain("inline-flex");
    expect(compact.getAttribute("class")).toContain("rounded-full");
    expect(sidebar.getAttribute("class")).not.toContain("rounded-full");
  });

  test("scan progress stays indeterminate until the probe total is known", async () => {
    await startScanInShell("/app/printers");

    for (const region of discoveryRegions()) {
      expect(region.querySelector("progress")?.hasAttribute("value")).toBe(false);
      expect(within(region).queryByText("0 / 0")).toBeNull();
      expect(within(region).getByText("In progress…")).toBeTruthy();
    }
  });

  // A USB-only scope resolves to no scan targets, so the server sends
  // `prepared` with a zero probe total and never sends a `progress` event.
  // The readout has to stay honest for the whole life of that scan.
  test("a USB-only scan never claims to be preparing or to have zero of zero probes", async () => {
    const { stream } = await startScanInShell("/app/printers");
    act(() => { stream.emit("prepared", { targets: [], skipped: [], total_probes: 0 }); });

    for (const region of discoveryRegions()) {
      expect(within(region).queryByText("Preparing…")).toBeNull();
      expect(within(region).queryByText("0 / 0")).toBeNull();
      expect(within(region).getByText("In progress…")).toBeTruthy();
    }
  });

  // A rescan started while one is already running resets the probe total to
  // zero without unmounting the bar, which is the only way the determinate
  // to indeterminate direction is ever exercised.
  test("a rescan drops the probe total back to an indeterminate bar", async () => {
    const { stream } = await startScanInShell("/app/printers");
    act(() => { stream.emit("progress", { completed: 312, total: 508 }); });
    for (const region of discoveryRegions()) {
      expect(region.querySelector("progress")?.getAttribute("value")).toBe("312");
    }

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });

    for (const region of discoveryRegions()) {
      expect(region.querySelector("progress")?.hasAttribute("value")).toBe(false);
    }
  });

  test("announces a scan starting and ending without announcing every probe", async () => {
    const { view, stream } = await startScanInShell("/app/printers");
    const announcers = [...view.container.querySelectorAll('[aria-live="polite"].sr-only')];
    expect(announcers).toHaveLength(2);
    for (const announcer of announcers) {
      expect(announcer.textContent).toBe("Scanning printers");
    }

    act(() => { stream.emit("progress", { completed: 312, total: 508 }); });
    for (const announcer of announcers) {
      expect(announcer.textContent).toBe("Scanning printers");
    }
    // The ticking readout must never sit inside a live region, or every probe
    // is announced on its own.
    for (const region of discoveryRegions()) {
      expect(region.closest("[aria-live]")).toBeNull();
    }

    act(() => { stream.emit("completed", {}); });
    expect(screen.queryAllByRole("region", { name: "Printer discovery" })).toHaveLength(0);
    for (const announcer of announcers) {
      expect(announcer.textContent).toBe("");
    }
  });

  test("keeps the sidebar status pair anchored to the bottom while a scan runs", async () => {
    const { stream } = await startScanInShell("/app/jobs");
    act(() => { stream.emit("progress", { completed: 4, total: 508 }); });

    const status = screen.getAllByRole("status", { name: "Server status" }).find((section) => section.closest("aside"));
    const anchored = status?.parentElement;
    expect(anchored?.getAttribute("class")).toContain("mt-auto");
    expect(anchored?.parentElement?.tagName).toBe("ASIDE");
    expect(anchored?.nextElementSibling).toBeNull();
    expect(status?.previousElementSibling?.getAttribute("aria-label")).toBe("Printer discovery");
  });

  test("an arriving print job shows in both responsive status variants on any page", async () => {
    FakeEventSource.instances = [];
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    renderAt("/app/profiles");
    const statusStream = FakeEventSource.forUrl("/api/status/events");

    act(() => statusStream?.emit("status", {
      virtual_printer: receiving,
      jobs_processed: 0,
      config_path: "/tmp/printers.toml",
    }));

    for (const region of jobRegions()) {
      expect(within(region).getByText("Incoming print job")).toBeTruthy();
      expect(within(region).getByRole("link", { name: "View" }).getAttribute("href")).toBe("/app/jobs");
      // No readout beside the bar: nothing measures a job's size, so the only
      // thing it could say is that the job is in progress, which the label
      // and the indeterminate bar already say.
      expect(region.textContent).toBe("Incoming print jobView");
    }
  });

  test("refreshes printer inventory once when server status reconnects", async () => {
    FakeEventSource.instances = [];
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    let printerRequests = 0;
    let resolveInitialInventory!: (response: Response) => void;
    globalThis.fetch = ((input: RequestInfo | URL) => {
      if (String(input) === "/api/printers/list") {
        printerRequests += 1;
        if (printerRequests === 1) {
          return new Promise<Response>((resolve) => {
            resolveInitialInventory = resolve;
          });
        }
      }
      return Promise.resolve(new Response(JSON.stringify({ printers: [] }), {
        headers: { "content-type": "application/json" },
      }));
    }) as unknown as typeof globalThis.fetch;

    renderAt("/app/profiles");
    expect(printerRequests).toBe(1);
    const statusStream = FakeEventSource.forUrl("/api/status/events");

    act(() => statusStream?.emit("error", null));
    act(() => statusStream?.emit("status", {
      virtual_printer: ready,
      jobs_processed: 0,
      config_path: "/tmp/printers.toml",
    }));
    expect(printerRequests).toBe(1);

    resolveInitialInventory(new Response(JSON.stringify({ printers: [] }), {
      headers: { "content-type": "application/json" },
    }));
    await act(async () => { await flush(); });
    expect(printerRequests).toBe(2);

    act(() => statusStream?.emit("status", {
      virtual_printer: ready,
      jobs_processed: 1,
      config_path: "/tmp/printers.toml",
    }));
    await act(async () => { await flush(); });
    expect(printerRequests).toBe(2);
  });

  test("shows no job block while the virtual printer is idle or absent", async () => {
    virtualPrinter = ready;
    await renderShell("/app/jobs");
    expect(screen.queryAllByRole("region", { name: "Print job" })).toHaveLength(0);

    cleanup();
    virtualPrinter = null;
    await renderShell("/app/jobs");
    expect(screen.queryAllByRole("region", { name: "Print job" })).toHaveLength(0);
  });

  // Nothing announces the size of an incoming job — not the status payload and
  // not the current-job endpoint — so the bar has no value to show and must
  // never invent one.
  test("an incoming job's bar stays indeterminate, with no invented total", async () => {
    virtualPrinter = receiving;
    await renderShell("/app/jobs");

    for (const region of jobRegions()) {
      const bar = region.querySelector("progress");
      expect(bar?.hasAttribute("value")).toBe(false);
      expect(bar?.hasAttribute("max")).toBe(false);
    }
  });

  test("renders the job's sidebar variant as a card and its compact variant as an inline pill", async () => {
    virtualPrinter = receiving;
    await renderShell("/app/jobs");

    const [sidebar, compact] = jobRegions();
    expect(sidebar.getAttribute("class")).toContain("text-sm");
    expect(compact.getAttribute("class")).toContain("text-xs");
    expect(compact.getAttribute("class")).toContain("inline-flex");
    expect(compact.getAttribute("class")).toContain("rounded-full");
    expect(sidebar.getAttribute("class")).not.toContain("rounded-full");
  });

  // Both activities are independent and can overlap. The order they appear in
  // is fixed rather than most-recent-first, so neither pill moves under the
  // reader when the other one comes or goes.
  test("stacks a scan and an incoming job in a fixed order in both variants", async () => {
    virtualPrinter = receiving;
    const { stream } = await startScanInShell("/app/jobs");
    act(() => { stream.emit("progress", { completed: 4, total: 508 }); });

    const [sidebarScan, compactScan] = discoveryRegions();
    const [sidebarJob, compactJob] = jobRegions();
    const position = (region: Element) => [...(region.parentElement?.children ?? [])].indexOf(region);
    for (const [scan, job] of [[sidebarScan, sidebarJob], [compactScan, compactJob]]) {
      expect(scan!.parentElement).toBe(job!.parentElement);
      expect(position(scan!)).toBeLessThan(position(job!));
    }

    // The status block stays where it was: last, and still anchored to the
    // bottom of the sidebar column with two blocks above it rather than one.
    const status = screen.getAllByRole("status", { name: "Server status" }).find((section) => section.closest("aside"));
    expect(status?.parentElement?.getAttribute("class")).toContain("mt-auto");
    expect(status?.nextElementSibling).toBeNull();
    expect(status?.previousElementSibling?.getAttribute("aria-label")).toBe("Print job");
  });

  // The header is a thin bar and the pills are inline, so they share a row
  // wherever there is width for one — only wrapping to a second line on a
  // narrow screen. The sidebar cards are full width and can only stack, so
  // they stay direct children of the anchored column.
  test("lays the compact pills out as a wrapping row and the sidebar pills as a stack", async () => {
    virtualPrinter = receiving;
    const { stream } = await startScanInShell("/app/printers");
    act(() => { stream.emit("progress", { completed: 4, total: 508 }); });

    const [sidebarScan, compactScan] = discoveryRegions();
    const [sidebarJob, compactJob] = jobRegions();
    expect(compactScan.parentElement).toBe(compactJob.parentElement);
    expect(compactScan.parentElement?.getAttribute("class")).toContain("flex-wrap");
    expect(sidebarScan.parentElement).toBe(sidebarJob.parentElement);
    expect(sidebarScan.parentElement?.getAttribute("class")).toContain("mt-auto");
  });

  test("announces an arriving job, and both activities while they overlap", async () => {
    virtualPrinter = receiving;
    const view = await renderShell("/app/jobs");

    expect(announcers(view.container)).toHaveLength(2);
    for (const announcer of announcers(view.container)) {
      expect(announcer.textContent).toBe("Incoming print job");
    }
    // The pill itself must stay out of any live region, or a screen reader
    // reads the whole shell again on every unchanged status event.
    for (const region of jobRegions()) {
      expect(region.closest("[aria-live]")).toBeNull();
    }

    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });

    for (const announcer of announcers(view.container)) {
      expect(announcer.textContent).toBe("Scanning printers. Incoming print job");
    }
  });

  // The provider keeps the last status snapshot while the server is
  // unreachable, so an unguarded pill would go on claiming a job is arriving
  // at a server nobody can reach — and would say so directly above a status
  // block reading "Disconnected".
  test("hides an incoming job while disconnected and restores it after reconnection", async () => {
    virtualPrinter = receiving;
    await renderShell("/app/jobs");
    expect(screen.getAllByRole("region", { name: "Print job" })).toHaveLength(2);

    const statusStream = FakeEventSource.forUrl("/api/status/events");
    act(() => statusStream?.emit("error", null));

    expect(screen.getAllByText("Disconnected")).toHaveLength(2);
    expect(screen.queryAllByRole("region", { name: "Print job" })).toHaveLength(0);

    act(() => statusStream?.emit("status", {
      virtual_printer: receiving,
      jobs_processed: 0,
      config_path: "/tmp/printers.toml",
    }));

    for (const region of jobRegions()) {
      expect(within(region).getByText("Incoming print job")).toBeTruthy();
    }
  });

  test("drops the job block and its announcement once the job stops arriving", async () => {
    virtualPrinter = receiving;
    const view = await renderShell("/app/jobs");
    expect(screen.getAllByRole("region", { name: "Print job" })).toHaveLength(2);

    act(() => FakeEventSource.forUrl("/api/status/events")?.emit("status", {
      virtual_printer: ready,
      jobs_processed: 0,
      config_path: "/tmp/printers.toml",
    }));

    expect(screen.queryAllByRole("region", { name: "Print job" })).toHaveLength(0);
    for (const announcer of announcers(view.container)) {
      expect(announcer.textContent).toBe("");
    }
  });
});
