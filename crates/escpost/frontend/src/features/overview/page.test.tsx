import { afterEach, describe, expect, test } from "bun:test";
import { act, cleanup, render, screen, within } from "@testing-library/preact";
import { AppDataProvider } from "../../app/data";
import { PrinterInventoryProvider } from "../../app/printer-inventory-data";
import { ServerStatusProvider } from "../../app/server-status-data";
import type { ServerStatusSnapshot } from "../../api/types";
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

function renderOverview(
  printers: unknown[] = [],
  warning: string | null = null,
  emitInventory = true,
  status: ServerStatusSnapshot = { virtual_printer: { state: "receiving", address: "127.0.0.1:9100" }, jobs_processed: 7, config_path: "" },
) {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  render(<ServerStatusProvider><PrinterInventoryProvider><AppDataProvider><OverviewPage /></AppDataProvider></PrinterInventoryProvider></ServerStatusProvider>);
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("message", status));
  const source = FakeEventSource.forUrl("/api/printers/list/events")!;
  if (emitInventory) act(() => source.emit("message", { updated_at: "2026-08-26T14:32:10Z", warning, printers }));
  return source;
}

describe("OverviewPage", () => {
  test("keeps the theme-aware branding and dashboard layout contracts", () => {
    renderOverview();
    const page = screen.getByRole("heading", { name: "Overview" }).closest("section")!;
    expect(page.getAttribute("class")).toContain("mx-auto");
    expect(page.getAttribute("class")).toContain("pt-6");
    expect(page.getAttribute("class")).not.toContain("my-auto");
    const logo = screen.getByRole("img", { name: "ESCPost" });
    expect(logo.getAttribute("src")).toContain("logo_light");
    expect(logo.parentElement?.querySelector("source")?.getAttribute("srcset")).toContain("logo_dark");
    for (const label of ["Jobs processed", "Printers", "Virtual printer"]) {
      const card = screen.getByRole("region", { name: label });
      expect(card.getAttribute("class")).toContain("text-center");
      expect(card.querySelector("h2")?.getAttribute("class")).toContain("text-left");
    }
  });
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

  test("distinguishes cold monitor and disconnected monitor without a snapshot", () => {
    const source = renderOverview([], null, false);
    expect(screen.getByText("Connecting to printer monitor…")).toBeTruthy();
    act(() => source.emit("error", {}));
    expect(screen.getByText("Unable to connect; retrying automatically.")).toBeTruthy();
  });

  test("keeps snapshot facts visible and marks them stale while reconnecting", () => {
    const source = renderOverview([
      { name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } },
    ]);
    act(() => source.emit("error", {}));
    expect(screen.getByText("1 configured")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("Showing stale printer data; reconnecting automatically.");
  });

  test("renders the virtual-printer absence, zero counts, and configuration path", () => {
    renderOverview([], null, true, {
      virtual_printer: null,
      jobs_processed: 0,
      config_path: "/home/dev/.config/escpost/printers.toml",
    });
    const printers = screen.getByRole("region", { name: "Printers" });
    expect(within(printers).getByText("0 configured")).toBeTruthy();
    expect(within(printers).queryByText("0 connected")).toBeNull();
    expect(within(printers).queryByText("0 unavailable")).toBeNull();
    expect(screen.getByText("Not running")).toBeTruthy();
    const path = screen.getByText("/home/dev/.config/escpost/printers.toml");
    expect(path.getAttribute("class")).toContain("font-mono");
    expect(path.parentElement?.textContent).toBe("Configuration /home/dev/.config/escpost/printers.toml");
  });
});
