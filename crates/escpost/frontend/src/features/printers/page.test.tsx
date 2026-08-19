import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, render, screen } from "@testing-library/preact";
import { AppDataProvider } from "../../app/data";
import { PrintersPage } from "./page";

const status = { virtual_printer: null, jobs_processed: 0 };
const printer = {
  name: "Kitchen",
  transport: "network",
  availability: "connected",
  profile: "REFERENCE",
  connection: { type: "network", host: "10.0.0.8", port: 9100 },
};

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function renderPage(fetch: typeof globalThis.fetch) {
  globalThis.fetch = fetch;
  return render(<AppDataProvider><PrintersPage /></AppDataProvider>);
}

afterEach(() => {
  cleanup();
  jest.useRealTimers();
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
      jest.advanceTimersByTime(5_000);
      await Promise.resolve();
    });
    expect(await screen.findAllByText("Bar")).toHaveLength(2);

    await act(async () => {
      jest.advanceTimersByTime(5_000);
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
});
