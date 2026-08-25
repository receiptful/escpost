import { afterEach, describe, expect, jest, test } from "bun:test";
import { addPrinter, ApiRequestError, getPrinters } from "./client";

const originalFetch = globalThis.fetch;

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

afterEach(() => {
  globalThis.fetch = originalFetch;
});

describe("API client", () => {
  test("sends JSON-only no-store requests and forwards cancellation", async () => {
    const controller = new AbortController();
    const fetch = jest.fn(() => Promise.resolve(json({ printers: [] })));
    globalThis.fetch = fetch as unknown as typeof globalThis.fetch;

    await getPrinters("network", controller.signal);

    const [path, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(path).toBe("/api/printers/list?transport=network");
    expect(new Headers(init.headers).get("Accept")).toBe("application/json");
    expect(init.cache).toBe("no-store");
    expect(init.signal).toBe(controller.signal);
  });

  test("exposes structured API errors", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(json({
      error: { code: "printer_inventory_unavailable", message: "Printer inventory is unavailable." },
    }, 500))) as unknown as typeof globalThis.fetch;

    try {
      await getPrinters();
      throw new Error("expected getPrinters to reject");
    } catch (error) {
      expect(error).toBeInstanceOf(ApiRequestError);
      expect(error).toMatchObject({ status: 500, code: "printer_inventory_unavailable", message: "Printer inventory is unavailable." });
    }
  });

  test("rejects an HTML response with an unexpected-response API error", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(new Response("<html></html>", {
      status: 200,
      headers: { "content-type": "text/html" },
    }))) as unknown as typeof globalThis.fetch;

    const failure = await getPrinters().catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiRequestError);
    expect(failure).toMatchObject({
      status: 200,
      code: "unexpected_response",
      message: "The server returned an unexpected response.",
    });
  });

  test("addPrinter posts a JSON body and returns the parsed response", async () => {
    const fetch = jest.fn(() => Promise.resolve(json({
      name: "kitchen",
      transport: "network",
      profile: null,
      warnings: [],
    }, 201)));
    globalThis.fetch = fetch as unknown as typeof globalThis.fetch;

    const body = {
      name: "kitchen",
      profile: null,
      connection: { type: "network" as const, host: "10.42.0.71", port: 9100 },
    };
    const response = await addPrinter(body);

    const [path, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(path).toBe("/api/printers/add");
    expect(init.method).toBe("POST");
    expect(new Headers(init.headers).get("Content-Type")).toBe("application/json");
    expect(init.cache).toBe("no-store");
    expect(JSON.parse(init.body as string)).toEqual(body);
    expect(response).toEqual({ name: "kitchen", transport: "network", profile: null, warnings: [] });
  });

  test("addPrinter posts JSON and surfaces the API error code", async () => {
    globalThis.fetch = (async () => new Response(
      JSON.stringify({ error: { code: "printer_already_configured", message: "Printer kitchen already exists." } }),
      { status: 409, headers: { "content-type": "application/json" } },
    )) as unknown as typeof globalThis.fetch;

    const failure = await addPrinter({
      name: "kitchen",
      profile: null,
      connection: { type: "network", host: "10.42.0.71", port: 9100 },
    }).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiRequestError);
    expect((failure as ApiRequestError).code).toBe("printer_already_configured");
  });
});
