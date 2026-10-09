import { afterEach, describe, expect, it, vi } from "vitest";
import { DaemonClient } from "../src/daemon";

afterEach(() => vi.restoreAllMocks());

function respond(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

describe("DaemonClient", () => {
  it("narrows escpost's listing to the shape the extension uses", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      respond({
        printers: [
          {
            name: "TM-T20",
            transport: "usb",
            availability: "connected",
            profile: "NT-5890K",
            connection: { type: "usb", vendor_id: 1208, product_id: 514, serial_number: "B1203" },
          },
          {
            name: "Kitchen",
            transport: "network",
            availability: "unavailable",
            profile: null,
            connection: { type: "network", host: "192.0.2.50", port: 9100 },
          },
        ],
      }),
    );
    const client = new DaemonClient("http://127.0.0.1:9000", fetchMock as unknown as typeof fetch);

    await expect(client.printers()).resolves.toEqual([
      {
        id: "TM-T20",
        name: "TM-T20",
        transport: "usb",
        profile: "NT-5890K",
        status: "ready",
        device: { usbVendorId: 1208, usbProductId: 514, usbSerial: "B1203" },
      },
      {
        id: "Kitchen",
        name: "Kitchen",
        transport: "network",
        profile: null,
        status: "unavailable",
        device: { host: "192.0.2.50", port: 9100 },
      },
    ]);
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:9000/api/printers/list",
      expect.objectContaining({ method: "GET" }),
    );
  });

  it("posts a print job as JSON with base64 data", async () => {
    const fetchMock = vi.fn().mockResolvedValue(respond({ jobId: "job-1" }));
    const client = new DaemonClient("http://127.0.0.1:9000", fetchMock as unknown as typeof fetch);
    await expect(client.print("tm-t20", "G0A=")).resolves.toEqual({ jobId: "job-1" });
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(url).toBe("http://127.0.0.1:9000/api/print");
    expect(init.method).toBe("POST");
    expect(init.headers["Content-Type"]).toBe("application/json");
    expect(JSON.parse(init.body)).toEqual({ printer: "tm-t20", data: "G0A=" });
  });

  it("retries a transient failure and succeeds without the caller noticing", async () => {
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new TypeError("Failed to fetch"))
      .mockResolvedValueOnce(respond({ jobId: "job-2" }));
    const client = new DaemonClient("http://127.0.0.1:9000", fetchMock as unknown as typeof fetch, { backoffMs: 0 });
    await expect(client.print("tm-t20", "G0A=")).resolves.toEqual({ jobId: "job-2" });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("gives up as DAEMON_NOT_RUNNING with install guidance", async () => {
    const fetchMock = vi.fn().mockRejectedValue(new TypeError("Failed to fetch"));
    const client = new DaemonClient("http://127.0.0.1:9000", fetchMock as unknown as typeof fetch, { backoffMs: 0 });
    await expect(client.printers()).rejects.toMatchObject({ code: "DAEMON_NOT_RUNNING" });
    await expect(client.printers()).rejects.toThrow(/escpost is not running/);
  });

  it("does not retry a 4xx, which will never succeed on repeat", async () => {
    const fetchMock = vi.fn().mockResolvedValue(respond({ error: { code: "PRINTER_NOT_FOUND", message: "no" } }, 404));
    const client = new DaemonClient("http://127.0.0.1:9000", fetchMock as unknown as typeof fetch, { backoffMs: 0 });
    await expect(client.print("nope", "G0A=")).rejects.toMatchObject({ code: "PRINTER_NOT_FOUND" });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("holds no connection state between calls", async () => {
    // mockImplementation, not mockResolvedValue: a real fetch returns a fresh Response
    // per call, and a Response body can only be read once.
    const fetchMock = vi.fn().mockImplementation(() => Promise.resolve(respond({ printers: [] })));
    const client = new DaemonClient("http://127.0.0.1:9000", fetchMock as unknown as typeof fetch);
    await client.printers();
    await client.printers();
    expect(Object.keys(client)).not.toContain("socket");
  });
});

describe("availability", () => {
  it("counts a plain-text health response as running", async () => {
    // /health answers "ok", not JSON. Parsing it as JSON threw, and the
    // extension told the user to start an escpost that was already running.
    const fetchMock = vi.fn().mockResolvedValue(new Response("ok", { status: 200 }));
    const client = new DaemonClient("http://127.0.0.1:9000", fetchMock as unknown as typeof fetch);

    await expect(client.available()).resolves.toBe(true);
    expect(fetchMock).toHaveBeenCalledWith("http://127.0.0.1:9000/health", expect.objectContaining({ method: "GET" }));
  });

  it("is false when nothing answers", async () => {
    const fetchMock = vi.fn().mockRejectedValue(new TypeError("Failed to fetch"));
    const client = new DaemonClient("http://127.0.0.1:9000", fetchMock as unknown as typeof fetch, { backoffMs: 0 });

    await expect(client.available()).resolves.toBe(false);
  });
});
