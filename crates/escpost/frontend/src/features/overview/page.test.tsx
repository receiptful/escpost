import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, render, screen } from "@testing-library/preact";
import { AppDataProvider } from "../../app/data";
import { OverviewPage } from "./page";

function json(body: unknown) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

afterEach(cleanup);

describe("OverviewPage", () => {
  test("derives printer counts and renders virtual printer facts", async () => {
    globalThis.fetch = ((input: RequestInfo | URL) => {
      if (String(input) === "/api/status") {
        return Promise.resolve(json({
          virtual_printer: { state: "receiving", address: "127.0.0.1:9100" },
          jobs_processed: 7,
        }));
      }
      return Promise.resolve(json({
        printers: [
          { name: "Kitchen", transport: "network", availability: "connected", profile: "REFERENCE", connection: { type: "network", host: "10.0.0.8", port: 9100 } },
          { name: "Counter", transport: "usb", availability: "unavailable", profile: null, connection: { type: "usb", vendor_id: 4660, product_id: 22136, bus: null, address: null, manufacturer: null, product: null, serial_number: null, interface_number: 0, out_endpoints: [1], in_endpoints: [] } },
        ],
      }));
    }) as typeof globalThis.fetch;

    render(<AppDataProvider><OverviewPage /></AppDataProvider>);
    expect(await screen.findByText("2 configured")).toBeTruthy();
    expect(screen.getByText("1 connected")).toBeTruthy();
    expect(screen.getByText("1 unavailable")).toBeTruthy();
    expect(screen.getByText("Ready")).toBeTruthy();
    expect(screen.getByText("Receiving")).toBeTruthy();
    expect(screen.getByText("127.0.0.1:9100")).toBeTruthy();
    expect(screen.getByText("7")).toBeTruthy();
  });

  test("renders Not running when no virtual printer is configured", async () => {
    globalThis.fetch = (() => Promise.resolve(json({ virtual_printer: null, jobs_processed: 0 }))) as unknown as typeof globalThis.fetch;
    render(<AppDataProvider><OverviewPage /></AppDataProvider>);
    expect(await screen.findByText("Not running")).toBeTruthy();
  });
});
