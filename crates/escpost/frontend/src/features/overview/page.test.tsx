import { afterEach, describe, expect, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/preact";
import { AppDataProvider, useAppData } from "../../app/data";
import { OverviewPage } from "./page";

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

afterEach(cleanup);

function OverviewWithRefresh() {
  const { refreshPrinters } = useAppData();
  return <><button type="button" onClick={() => void refreshPrinters()}>Refresh inventory</button><OverviewPage /></>;
}

describe("OverviewPage", () => {
  test("top-aligns the dashboard beneath a theme-aware ESCPost logo", () => {
    globalThis.fetch = (() => new Promise<Response>(() => {})) as unknown as typeof globalThis.fetch;
    const view = render(<AppDataProvider><OverviewPage /></AppDataProvider>);
    const page = view.container.querySelector("section");

    expect(page?.getAttribute("class")).toContain("mx-auto");
    expect(page?.getAttribute("class")).toContain("pt-6");
    expect(page?.getAttribute("class")).not.toContain("my-auto");
    expect(page?.getAttribute("class")).toContain("max-w-7xl");

    const logo = screen.getByRole("img", { name: "ESCPost" });
    expect(logo.getAttribute("src")).toContain("logo_light");
    const darkLogo = logo.parentElement?.querySelector("source");
    expect(darkLogo?.getAttribute("media")).toBe("(prefers-color-scheme: dark)");
    expect(darkLogo?.getAttribute("srcset")).toContain("logo_dark");
  });

  test("derives printer counts and renders virtual printer facts", async () => {
    globalThis.fetch = ((input: RequestInfo | URL) => {
      if (String(input) === "/api/status") {
        return Promise.resolve(json({
          virtual_printer: { state: "receiving", address: "127.0.0.1:9100" },
          jobs_processed: 7,
        }));
      }
      if (String(input) === "/api/profiles/list") {
        return Promise.resolve(json({ profiles: [] }));
      }
      return Promise.resolve(json({
        printers: [
          { name: "Kitchen", transport: "network", availability: "connected", profile: "REFERENCE", connection: { type: "network", host: "10.0.0.8", port: 9100 } },
          { name: "Counter", transport: "usb", availability: "unavailable", profile: null, connection: { type: "usb", vendor_id: 4660, product_id: 22136, bus: null, address: null, manufacturer: null, product: null, serial_number: null, interface_number: 0, out_endpoints: [1], in_endpoints: [] } },
        ],
      }));
    }) as typeof globalThis.fetch;

    render(<AppDataProvider><OverviewPage /></AppDataProvider>);
    const printers = await screen.findByRole("region", { name: "Printers" });
    expect(await within(printers).findByText("2 configured")).toBeTruthy();
    expect(within(printers).getByText("1 connected")).toBeTruthy();
    expect(within(printers).getByText("1 unavailable")).toBeTruthy();

    const virtualPrinter = screen.getByRole("region", { name: "Virtual printer" });
    expect(within(virtualPrinter).getByText("Receiving")).toBeTruthy();
    expect(within(virtualPrinter).getByText("127.0.0.1:9100")).toBeTruthy();

    const jobs = screen.getByRole("region", { name: "Jobs processed" });
    expect(within(jobs).getByText("7")).toBeTruthy();
    expect(screen.queryByRole("region", { name: "Printer availability" })).toBeNull();
  });

  test("left-aligns card headings while centering card values", async () => {
    globalThis.fetch = ((input: RequestInfo | URL) => String(input) === "/api/status"
      ? Promise.resolve(json({ virtual_printer: null, jobs_processed: 1 }))
      : Promise.resolve(json({
        printers: [
          { name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } },
        ],
      }))) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><OverviewPage /></AppDataProvider>);
    const cards = await Promise.all([
      screen.findByRole("region", { name: "Jobs processed" }),
      screen.findByRole("region", { name: "Printers" }),
      screen.findByRole("region", { name: "Virtual printer" }),
    ]);

    for (const card of cards) {
      expect(card.getAttribute("class")).toContain("text-center");
      expect(within(card).getByRole("heading").getAttribute("class")).toContain("text-left");
    }
    expect((await screen.findByText("1 connected")).parentElement?.getAttribute("class")).toContain("justify-center");
  });

  test("renders Not running when no virtual printer is configured", async () => {
    globalThis.fetch = ((input: RequestInfo | URL) => String(input) === "/api/profiles/list"
      ? Promise.resolve(json({ profiles: [] }))
      : Promise.resolve(json({ virtual_printer: null, jobs_processed: 0 }))) as unknown as typeof globalThis.fetch;
    render(<AppDataProvider><OverviewPage /></AppDataProvider>);
    expect(await screen.findByText("Not running")).toBeTruthy();
  });

  test("omits printer availability tags whose count is zero", async () => {
    globalThis.fetch = ((input: RequestInfo | URL) => String(input) === "/api/status"
      ? Promise.resolve(json({ virtual_printer: null, jobs_processed: 0 }))
      : Promise.resolve(json({ printers: [] }))) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><OverviewPage /></AppDataProvider>);
    const printers = await screen.findByRole("region", { name: "Printers" });
    expect(await within(printers).findByText("0 configured")).toBeTruthy();
    expect(within(printers).queryByText("0 connected")).toBeNull();
    expect(within(printers).queryByText("0 unavailable")).toBeNull();
  });

  test("shows inventory loading and error states instead of zero counts without printer data", async () => {
    let resolveInventory!: (response: Response) => void;
    globalThis.fetch = ((input: RequestInfo | URL) => String(input) === "/api/status"
      ? Promise.resolve(json({ virtual_printer: null, jobs_processed: 0 }))
      : String(input) === "/api/profiles/list"
        ? Promise.resolve(json({ profiles: [] }))
        : new Promise<Response>((resolve) => { resolveInventory = resolve; })) as unknown as typeof globalThis.fetch;

    const view = render(<AppDataProvider><OverviewPage /></AppDataProvider>);
    expect(await screen.findByText("Printer inventory loading…")).toBeTruthy();
    expect(screen.queryByText("0 configured")).toBeNull();

    await resolveInventory(json({ error: { code: "printer_inventory_unavailable", message: "Printer inventory is unavailable." } }, 500));
    expect(await screen.findByText("Printer inventory is unavailable.")).toBeTruthy();
    expect(screen.queryByText("0 configured")).toBeNull();
    view.unmount();
  });

  test("keeps factual printer counts visible when a refresh leaves stale inventory", async () => {
    const inventories = [
      json({ printers: [{ name: "Kitchen", transport: "network", availability: "connected", profile: null, connection: { type: "network", host: "10.0.0.8", port: 9100 } }] }),
      json({ error: { code: "printer_inventory_unavailable", message: "Printer inventory is unavailable." } }, 500),
    ];
    globalThis.fetch = ((input: RequestInfo | URL) => String(input) === "/api/status"
      ? Promise.resolve(json({ virtual_printer: null, jobs_processed: 0 }))
      : String(input) === "/api/profiles/list"
        ? Promise.resolve(json({ profiles: [] }))
        : Promise.resolve(inventories.shift()!)) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><OverviewWithRefresh /></AppDataProvider>);
    expect(await screen.findByText("1 configured")).toBeTruthy();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Refresh inventory" })); });
    expect(await screen.findByText("Showing cached printer data. Printer inventory is unavailable.")).toBeTruthy();
    expect(screen.getByText("1 configured")).toBeTruthy();
    expect(screen.getByText("1 connected")).toBeTruthy();
  });
});
