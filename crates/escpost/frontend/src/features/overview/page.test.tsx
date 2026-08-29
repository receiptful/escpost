import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/preact";
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
const originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
afterEach(() => {
  cleanup();
  globalThis.EventSource = originalEventSource;
  if (originalClipboard) Object.defineProperty(navigator, "clipboard", originalClipboard);
  else Reflect.deleteProperty(navigator, "clipboard");
});

function renderOverview(
  printers: unknown[] = [],
  warning: string | null = null,
  emitInventory = true,
  status: ServerStatusSnapshot = { virtual_printer: { state: "receiving", address: "127.0.0.1:9100" }, jobs_processed: 7, config_path: "" },
) {
  FakeEventSource.instances = [];
  globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
  render(<ServerStatusProvider><PrinterInventoryProvider><OverviewPage /></PrinterInventoryProvider></ServerStatusProvider>);
  act(() => FakeEventSource.forUrl("/api/status/events")?.emit("message", status));
  const source = FakeEventSource.forUrl("/api/printers/list/events")!;
  if (emitInventory) act(() => source.emit("message", { updated_at: "2026-08-26T14:32:10Z", warning, printers }));
  return source;
}

describe("OverviewPage", () => {
  test("keeps the theme-aware branding and groups virtual-printer facts", () => {
    renderOverview();
    const page = screen.getByRole("heading", { name: "Overview" }).closest("section")!;
    expect(page.getAttribute("class")).toContain("mx-auto");
    expect(page.getAttribute("class")).toContain("pt-6");
    expect(page.getAttribute("class")).not.toContain("my-auto");
    const logo = screen.getByRole("img", { name: "ESCPost" });
    expect(logo.getAttribute("src")).toContain("logo_light");
    expect(logo.parentElement?.querySelector("source")?.getAttribute("srcset")).toContain("logo_dark");
    for (const label of ["Configured printers", "Virtual printer"]) {
      const card = screen.getByRole("region", { name: label });
      expect(card.getAttribute("class")).toContain("text-center");
    }
    expect(screen.queryByRole("region", { name: "Jobs processed" })).toBeNull();

    const virtualPrinter = within(screen.getByRole("region", { name: "Virtual printer" }));
    expect(virtualPrinter.getByText("Receiving").getAttribute("class")).toContain("badge");
    expect(virtualPrinter.getByText("7")).toBeTruthy();
    expect(virtualPrinter.getByText("jobs processed this session")).toBeTruthy();
    expect(virtualPrinter.getByText("127.0.0.1").tagName).toBe("CODE");
  });

  test("switches the card grid to two columns only at the xl breakpoint", () => {
    renderOverview();
    const cards = screen.getByRole("region", { name: "Virtual printer" }).parentElement!;
    expect(cards.getAttribute("class")).toContain("xl:grid-cols-2");
    expect(cards.getAttribute("class")).not.toContain("auto-fit");
    const cardBlock = cards.parentElement!;
    expect(cardBlock.getAttribute("class")).toContain("mx-auto");
    expect(cardBlock.getAttribute("class")).toContain("max-w-[54rem]");
  });

  test("renders both dashboard cards with the same metric and label layout", () => {
    renderOverview([
      { name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } },
      { name: "Counter", transport: "network", availability: "unavailable", profile: null, connection: { type: "network", host: "10.0.0.9", port: 9100 } },
    ]);
    const printers = within(screen.getByRole("region", { name: "Configured printers" }));
    const virtualPrinter = within(screen.getByRole("region", { name: "Virtual printer" }));
    const printerMetric = printers.getByText("2");
    const printerLabel = printers.getByText("Configured printers");
    const jobsMetric = virtualPrinter.getByText("7");
    const jobsLabel = virtualPrinter.getByText("jobs processed this session");

    expect(printerMetric.getAttribute("class")).toBe(jobsMetric.getAttribute("class"));
    expect(printerLabel.getAttribute("class")).toBe(jobsLabel.getAttribute("class"));
    expect(printerMetric.compareDocumentPosition(printerLabel) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
  });

  test("keeps visible card titles in the shared headers opposite their status badges", () => {
    renderOverview([
      { name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } },
    ]);
    const printers = screen.getByRole("region", { name: "Configured printers" });
    const virtualPrinter = screen.getByRole("region", { name: "Virtual printer" });
    const printerHeader = within(printers).getByRole("heading", { name: "Printers" }).parentElement as HTMLElement;
    const virtualHeader = within(virtualPrinter).getByRole("heading", { name: "Virtual printer" }).parentElement as HTMLElement;

    expect(within(printerHeader).getByRole("heading", { name: "Printers" })).toBeTruthy();
    expect(within(printerHeader).getByText("1 connected")).toBeTruthy();
    expect(within(virtualHeader).getByRole("heading", { name: "Virtual printer" })).toBeTruthy();
    expect(within(virtualHeader).getByText("Receiving")).toBeTruthy();
  });

  test("uses the footer only for virtual-printer connection details", () => {
    renderOverview();
    const printers = screen.getByRole("region", { name: "Configured printers" });
    const virtualPrinter = screen.getByRole("region", { name: "Virtual printer" });

    expect(printers.querySelector("footer")).toBeNull();
    const footer = virtualPrinter.querySelector("footer")!;
    expect(footer).toBeTruthy();
    expect(within(footer).getByRole("group", { name: "Virtual printer IP" })).toBeTruthy();
    expect(within(footer).getByRole("group", { name: "Virtual printer port" })).toBeTruthy();
  });

  test("links each full card surface to its detail page", () => {
    renderOverview();
    const destinations = [
      { card: "Configured printers", link: "Open Printers", href: "/printers" },
      { card: "Virtual printer", link: "Open Virtual printer", href: "/jobs" },
    ];

    for (const destination of destinations) {
      const card = within(screen.getByRole("region", { name: destination.card }));
      const link = card.getByRole("link", { name: destination.link });
      expect(link.getAttribute("href")).toBe(destination.href);
      expect(link.getAttribute("class")).toContain("absolute");
      expect(link.getAttribute("class")).toContain("inset-0");
    }
  });

  test("derives printer counts from the inventory snapshot", () => {
    renderOverview([
      { name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } },
      { name: "Counter", transport: "network", availability: "unavailable", profile: null, connection: { type: "network", host: "10.0.0.9", port: 9100 } },
    ]);
    const printers = screen.getByRole("region", { name: "Configured printers" });
    const printerHeader = within(printers).getByRole("heading", { name: "Printers" }).parentElement as HTMLElement;
    expect(within(printers).getByText("2")).toBeTruthy();
    expect(within(printerHeader).getByText("1 connected")).toBeTruthy();
    expect(within(printerHeader).getByText("1 unavailable")).toBeTruthy();
    expect(screen.getByText("Receiving")).toBeTruthy();
  });

  test("copies the virtual printer IP and port independently", () => {
    const writeText = jest.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    renderOverview();

    const virtualPrinter = within(screen.getByRole("region", { name: "Virtual printer" }));
    expect(virtualPrinter.getByText("IP:")).toBeTruthy();
    expect(virtualPrinter.getByText("127.0.0.1")).toBeTruthy();
    expect(virtualPrinter.getByText("Port:")).toBeTruthy();
    expect(virtualPrinter.getByText("9100")).toBeTruthy();
    expect(virtualPrinter.getByRole("group", { name: "Virtual printer IP" }).getAttribute("class")).toContain("flex");
    expect(virtualPrinter.getByRole("group", { name: "Virtual printer port" }).getAttribute("class")).toContain("flex");

    const copyIp = virtualPrinter.getByRole("button", { name: "Copy virtual printer IP" });
    const copyPort = virtualPrinter.getByRole("button", { name: "Copy virtual printer port" });
    expect(copyIp.closest("a")).toBeNull();
    expect(copyPort.closest("a")).toBeNull();
    fireEvent.click(copyIp);
    fireEvent.click(copyPort);
    expect(writeText).toHaveBeenNthCalledWith(1, "127.0.0.1");
    expect(writeText).toHaveBeenNthCalledWith(2, "9100");
  });

  test("separates a bracketed IPv6 listener into a copyable IP and port", () => {
    const writeText = jest.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    renderOverview([], null, true, {
      virtual_printer: { state: "ready", address: "[::1]:9100" },
      jobs_processed: 0,
      config_path: "",
    });

    const virtualPrinter = within(screen.getByRole("region", { name: "Virtual printer" }));
    expect(virtualPrinter.getByText("::1")).toBeTruthy();
    expect(virtualPrinter.getByText("9100")).toBeTruthy();
    fireEvent.click(virtualPrinter.getByRole("button", { name: "Copy virtual printer IP" }));
    expect(writeText).toHaveBeenCalledWith("::1");
  });

  test("shows the backend inventory warning without hiding facts", () => {
    renderOverview([], "Monitor is catching up");
    expect(within(screen.getByRole("region", { name: "Configured printers" })).getByText("0")).toBeTruthy();
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
    expect(within(screen.getByRole("region", { name: "Configured printers" })).getByText("1")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("Showing stale printer data; reconnecting automatically.");
  });

  test("renders the virtual-printer absence, zero counts, and configuration path", () => {
    renderOverview([], null, true, {
      virtual_printer: null,
      jobs_processed: 0,
      config_path: "/home/dev/.config/escpost/printers.toml",
    });
    const printers = screen.getByRole("region", { name: "Configured printers" });
    expect(within(printers).getByText("0")).toBeTruthy();
    expect(within(printers).queryByText("0 connected")).toBeNull();
    expect(within(printers).queryByText("0 unavailable")).toBeNull();
    expect(screen.getByText("Not running")).toBeTruthy();
    expect(screen.getByText("Virtual printer is disabled.")).toBeTruthy();
    expect(screen.queryByText("No virtual printer is running.")).toBeNull();
    const path = screen.getByText("/home/dev/.config/escpost/printers.toml");
    expect(path.getAttribute("class")).toContain("font-mono");
    expect(path.parentElement?.textContent).toBe("Configuration /home/dev/.config/escpost/printers.toml");
  });
});
