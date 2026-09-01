import { expect, test } from "vitest";
import { DaemonClient, DaemonError } from "../src/daemon";
import { DaemonPortStore } from "../src/daemon-port";

class MemoryStorageArea {
  readonly values: Record<string, unknown> = {};

  async get(key: string): Promise<Record<string, unknown>> {
    return key in this.values ? { [key]: this.values[key] } : {};
  }

  async set(values: Record<string, unknown>): Promise<void> {
    Object.assign(this.values, values);
  }

  async remove(key: string): Promise<void> {
    delete this.values[key];
  }
}

const inventory = {
  updated_at: "2026-09-01T10:30:00Z",
  warning: "A probe is slow.",
  printers: [
    {
      name: "counter", transport: "usb", availability: "connected", profile: "80mm",
      connection: {
        type: "usb", vendor_id: 1046, product_id: 20497, bus: "001", address: 4,
        manufacturer: "Bixolon", product: "SRP-350", serial_number: "serial-7",
        interface_number: 2, out_endpoints: [1, 3], in_endpoints: [129],
      },
    },
    {
      name: "kitchen", transport: "network", availability: "unavailable", profile: null,
      connection: { type: "network", host: "192.0.2.7", port: 9100 },
    },
  ],
};

function response(body: unknown, status = 200): Response {
  return new Response(body === undefined ? null : JSON.stringify(body), { status });
}

function client(fetcher: typeof fetch, storage = new MemoryStorageArea()): {
  client: DaemonClient;
  ports: DaemonPortStore;
  storage: MemoryStorageArea;
} {
  const ports = new DaemonPortStore(storage);
  return { client: new DaemonClient(ports, fetcher), ports, storage };
}

test("discovers the next healthy loopback daemon, persists it, and returns complete filtered inventory", async () => {
  // Break caught: skipping bounded discovery, losing a configured connection
  // fact, or failing to encode the transport query hides usable printers.
  const calls: string[] = [];
  const { client: daemon, storage } = client(async (input) => {
    const url = String(input);
    calls.push(url);
    if (url === "http://127.0.0.1:9000/health") throw new TypeError("offline");
    if (url === "http://127.0.0.1:9001/health") return response({ ok: true });
    if (url === "http://127.0.0.1:9001/api/printers/list?transport=network") return response(inventory);
    throw new Error(`unexpected request ${url}`);
  });

  await expect(daemon.list("network")).resolves.toEqual(inventory);
  expect(calls).toEqual([
    "http://127.0.0.1:9000/health",
    "http://127.0.0.1:9001/health",
    "http://127.0.0.1:9001/api/printers/list?transport=network",
  ]);
  expect(storage.values).toEqual({ daemonBaseUrl: "http://127.0.0.1:9001" });
});

test("retries a list only after a cached port has a transport failure", async () => {
  // Break caught: not recovering from a daemon restart, or replaying a GET
  // after a server response whose outcome was already known.
  const calls: string[] = [];
  const { client: daemon, ports } = client(async (input) => {
    const url = String(input);
    calls.push(url);
    if (url === "http://127.0.0.1:9004/api/printers/list") throw new TypeError("connection reset");
    if (url === "http://127.0.0.1:9000/health") return response({ ok: true });
    if (url === "http://127.0.0.1:9000/api/printers/list") return response(inventory);
    throw new Error(`unexpected request ${url}`);
  });
  await ports.remember("http://127.0.0.1:9004");

  await expect(daemon.list()).resolves.toEqual(inventory);
  expect(calls).toEqual([
    "http://127.0.0.1:9004/api/printers/list",
    "http://127.0.0.1:9000/health",
    "http://127.0.0.1:9000/api/printers/list",
  ]);
});

test("does not rediscover after a list 4xx response", async () => {
  // Break caught: replaying a request rejected by the daemon can conceal a
  // bad caller request and sends traffic to an unrelated process.
  const calls: string[] = [];
  const { client: daemon, ports } = client(async (input) => {
    calls.push(String(input));
    return response({ error: "bad request" }, 400);
  });
  await ports.remember("http://127.0.0.1:9003");

  await expect(daemon.list()).rejects.toMatchObject({ code: "DAEMON_UNAVAILABLE" });
  expect(calls).toEqual(["http://127.0.0.1:9003/api/printers/list"]);
  await expect(ports.read()).resolves.toBe("http://127.0.0.1:9003");
});

test("posts exact raw bytes to the configured printer name", async () => {
  // Break caught: JSON/base64 conversion or an unencoded printer query
  // corrupts receipt bytes or changes the configured printer target.
  const calls: Array<[RequestInfo | URL, RequestInit | undefined]> = [];
  const { client: daemon, ports } = client(async (input, init) => {
    calls.push([input, init]);
    return response({ job_id: "job-17" });
  });
  await ports.remember("http://127.0.0.1:9000");

  await expect(daemon.print("counter & bar", new Uint8Array([0x1b, 0x40, 0xff]))).resolves.toEqual({ job_id: "job-17" });
  expect(calls).toHaveLength(1);
  expect(calls[0][0]).toBe("http://127.0.0.1:9000/api/print?printer=counter+%26+bar");
  expect(calls[0][1]).toMatchObject({
    method: "POST",
    headers: { "Content-Type": "application/octet-stream" },
    body: expect.any(Uint8Array),
  });
  expect(calls[0][1]?.body).toEqual(new Uint8Array([0x1b, 0x40, 0xff]));
});

test("does not replay a rejected print and invalidates the cached port", async () => {
  // Break caught: retrying an ambiguous print can duplicate a receipt after
  // the daemon accepted bytes but its connection closed before replying.
  const calls: string[] = [];
  const { client: daemon, ports } = client(async (input) => {
    calls.push(String(input));
    throw new TypeError("connection reset after upload");
  });
  await ports.remember("http://127.0.0.1:9002");

  await expect(daemon.print("counter", new Uint8Array([0x1b]))).rejects.toBeInstanceOf(DaemonError);
  expect(calls).toEqual(["http://127.0.0.1:9002/api/print?printer=counter"]);
  await expect(ports.read()).resolves.toBeNull();
});

test("does not replay a print after a 5xx response and invalidates its port", async () => {
  // Break caught: treating an HTTP failure as proof that no receipt reached
  // the printer permits duplicate physical receipt delivery.
  const calls: string[] = [];
  const { client: daemon, ports } = client(async (input) => {
    calls.push(String(input));
    return response({ error: "print failed" }, 500);
  });
  await ports.remember("http://127.0.0.1:9005");

  await expect(daemon.print("counter", new Uint8Array([0x1b]))).rejects.toMatchObject({ code: "DAEMON_UNAVAILABLE" });
  expect(calls).toEqual(["http://127.0.0.1:9005/api/print?printer=counter"]);
  await expect(ports.read()).resolves.toBeNull();
});
