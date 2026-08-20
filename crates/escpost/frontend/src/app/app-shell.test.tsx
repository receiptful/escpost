import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/preact";
import { LocationProvider } from "preact-iso";
import { locationStub } from "preact-iso/prerender";
import { App } from "../app";
import type { DiscoveryQuery } from "../api/discovery-stream";
import { AppDataProvider, useAppData } from "./data";
import { AppShell } from "./shell";

const originalFetch = globalThis.fetch;
const originalEventSource = globalThis.EventSource;

beforeEach(() => {
  globalThis.fetch = ((input: RequestInfo | URL) => Promise.resolve(new Response(JSON.stringify(
    String(input) === "/api/status"
      ? { virtual_printer: null, jobs_processed: 0 }
      : String(input) === "/api/jobs/current"
        ? { receiving: false, profile: "REFERENCE", error: null, job: null }
        : { printers: [] },
  ), { headers: { "content-type": "application/json" } }))) as unknown as typeof globalThis.fetch;
});

afterEach(() => {
  cleanup();
  globalThis.fetch = originalFetch;
  globalThis.EventSource = originalEventSource;
});

function renderAt(path: string) {
  locationStub(path);
  return render(<App />);
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

async function startScanInShell(path: string) {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  locationStub(path);
  const view = render(
    <AppDataProvider>
      <LocationProvider scope="/app">
        <AppShell><ScanProbe /></AppShell>
      </LocationProvider>
    </AppDataProvider>,
  );
  await act(async () => {});
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });
  return { view, stream: FakeEventSource.instances[0]! };
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

    const regions = screen.getAllByRole("region", { name: "Printer discovery" });
    expect(regions).toHaveLength(2);
    for (const region of regions) {
      expect(within(region).getByText("Scanning printers")).toBeTruthy();
      expect(within(region).getByText("312 / 508")).toBeTruthy();
      expect(within(region).getByRole("link", { name: "View" }).getAttribute("href")).toBe("/app/printers");
      const bar = region.querySelector("progress");
      expect(bar?.getAttribute("value")).toBe("312");
      expect(bar?.getAttribute("max")).toBe("508");
    }
    expect(regions.find((region) => region.closest("aside"))?.getAttribute("class")).toContain("text-sm");
    expect(regions.find((region) => region.closest("header"))?.getAttribute("class")).toContain("text-xs");
  });

  test("scan progress stays indeterminate until the probe total is known", async () => {
    await startScanInShell("/app/printers");

    for (const region of screen.getAllByRole("region", { name: "Printer discovery" })) {
      const bar = region.querySelector("progress");
      expect(bar?.hasAttribute("value")).toBe(false);
      expect(within(region).queryByText("0 / 0")).toBeNull();
      expect(within(region).getByText("Preparing…")).toBeTruthy();
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
});
