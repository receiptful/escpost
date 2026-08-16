import { afterEach, describe, expect, jest, test } from "bun:test";
import { ApiRequestError, getPrinters, getStatus } from "./client";

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

  test("rejects an HTML response instead of treating it as JSON", async () => {
    globalThis.fetch = jest.fn(() => Promise.resolve(new Response("<html></html>", {
      status: 200,
      headers: { "content-type": "text/html" },
    }))) as unknown as typeof globalThis.fetch;

    await expect(getStatus()).rejects.toMatchObject({ code: "unexpected_response" });
  });
});
