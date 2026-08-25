import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import type { DiscoveryQuery } from "../api/discovery-stream";
import { AppDataProvider, useAppData } from "./data";

const printerInventory = { printers: [] };

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function Probe() {
  const { printers } = useAppData();
  return <p>{printers.phase}</p>;
}

function FlashProbe() {
  const { printerFlashes } = useAppData();
  return <span data-testid="flashes">{JSON.stringify(printerFlashes)}</span>;
}

const scanQuery: DiscoveryQuery = { usb: true, network: true, subnets: [], port: 9100, timeoutMs: 1000 };

// The product ids of every USB failure the scan has reported, in the order
// the provider is holding them.
function ScanFailureProbe() {
  const { scan } = useAppData();
  return <span data-testid="failures">{scan.failures.map((failure) => failure.product_id).join(",")}</span>;
}

function ScanProbe() {
  const { startScan } = useAppData();
  return <button type="button" onClick={() => startScan(scanQuery)}>Scan</button>;
}

// Neither Bun's runtime nor the happy-dom registrator used by the test setup
// provides a global `EventSource`, so `startScan` (which calls the real
// `openDiscoveryStream`) is exercised against a small stand-in, the same way
// `discovery-stream.test.ts` stands in for it. Each instance stamps its own
// construction and closure with a shared monotonic counter, so a test can
// assert not just that a stream eventually closed but that it closed before
// the next one was constructed — the actual ordering guarantee that matters,
// since closing the `EventSource` is the only way a scan is cancelled
// server-side.
class FakeEventSource {
  static instances: FakeEventSource[] = [];
  private static sequence = 0;
  readonly url: string;
  readonly constructedAt: number;
  closed = false;
  closedAt: number | null = null;
  private readonly listeners = new Map<string, ((event: Event) => void)[]>();

  constructor(url: string) {
    this.url = url;
    this.constructedAt = FakeEventSource.sequence++;
    FakeEventSource.instances.push(this);
  }

  addEventListener(name: string, handler: (event: Event) => void) {
    const existing = this.listeners.get(name) ?? [];
    existing.push(handler);
    this.listeners.set(name, existing);
  }

  // Dispatches one named stream event, so a test can drive the provider the
  // way the server drives it.
  emit(name: string, payload: unknown) {
    for (const handler of this.listeners.get(name) ?? []) {
      handler(new MessageEvent(name, { data: JSON.stringify(payload) }));
    }
  }

  close() {
    this.closed = true;
    this.closedAt = FakeEventSource.sequence++;
  }
}

const originalEventSource = globalThis.EventSource;

// Steps `bun:test`'s fake clock forward and flushes the microtask queue, so
// a `.then`/`.finally` chain a timer kicked off has settled before the next
// assertion runs.
async function advanceTimers(milliseconds: number) {
  jest.advanceTimersByTime(milliseconds);
  // The fetch mock's promise chain (fetch -> response.json() -> refreshPrinters's
  // .then -> .finally) takes several microtask turns to settle; one
  // `await Promise.resolve()` is not enough to drain it, as the other tests
  // in this file (see the repeated awaits below) already demonstrate.
  for (let i = 0; i < 6; i += 1) {
    await Promise.resolve();
  }
}

afterEach(() => {
  cleanup();
  jest.useRealTimers();
  globalThis.EventSource = originalEventSource;
});

describe("AppDataProvider", () => {
  test("polls printer inventory ten seconds after each completed response", async () => {
    jest.useFakeTimers();
    let printerRequests = 0;
    let resolveInitialInventory!: (response: Response) => void;
    globalThis.fetch = jest.fn((input: RequestInfo | URL) => {
      printerRequests += 1;
      if (printerRequests === 1) {
        return new Promise<Response>((resolve) => {
          resolveInitialInventory = resolve;
        });
      }
      return Promise.resolve(json(printerInventory));
    }) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><Probe /></AppDataProvider>);
    expect(printerRequests).toBe(1);
    await act(async () => {
      resolveInitialInventory(json(printerInventory));
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByText("ready")).toBeTruthy();
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    await act(async () => { jest.advanceTimersByTime(9_999); });
    expect(printerRequests).toBe(1);
    await act(async () => {
      jest.advanceTimersByTime(1);
      await Promise.resolve();
    });
    expect(printerRequests).toBe(2);
  });

  test("requests printer inventory without requesting status", async () => {
    const fetch = jest.fn((_input: RequestInfo | URL) => Promise.resolve(json(printerInventory)));
    globalThis.fetch = fetch as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><Probe /></AppDataProvider>);
    expect(await screen.findByText("ready")).toBeTruthy();
    expect(fetch.mock.calls.map(([input]) => String(input))).toEqual(["/api/printers/list"]);
    expect(fetch.mock.calls.map(([input]) => String(input))).not.toContain("/api/status");
  });

  test("does not request profiles until a Profiles page mounts", async () => {
    const fetch = jest.fn((_input: RequestInfo | URL) => Promise.resolve(json(printerInventory)));
    globalThis.fetch = fetch as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><Probe /></AppDataProvider>);
    expect(await screen.findByText("ready")).toBeTruthy();
    expect(fetch.mock.calls.map(([input]) => String(input))).not.toContain("/api/profiles/list");
  });

  test("polling stops while the document is hidden and resumes when it is visible", async () => {
    jest.useFakeTimers();
    let calls = 0;
    globalThis.fetch = ((input: RequestInfo | URL) => {
      if (String(input) === "/api/printers/list") {
        calls += 1;
      }
      return Promise.resolve(json({ printers: [] }));
    }) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><FlashProbe /></AppDataProvider>);
    await act(async () => { await advanceTimers(0); });
    const initial = calls;

    Object.defineProperty(document, "visibilityState", { value: "hidden", configurable: true });
    document.dispatchEvent(new Event("visibilitychange"));
    await act(async () => { await advanceTimers(20_000); });
    expect(calls).toBe(initial);

    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
    document.dispatchEvent(new Event("visibilitychange"));
    await act(async () => {});
    expect(calls).toBe(initial + 1);
  });

  test("a printer that becomes unavailable is flagged as lost exactly once", async () => {
    jest.useFakeTimers();
    const availabilities = ["connected", "unavailable", "unavailable"];
    let poll = 0;
    globalThis.fetch = ((input: RequestInfo | URL) => {
      if (String(input) !== "/api/printers/list") {
        return Promise.resolve(json({ virtual_printer: null, jobs_processed: 0, config_path: "/tmp/printers.toml" }));
      }
      const availability = availabilities[Math.min(poll, availabilities.length - 1)];
      poll += 1;
      return Promise.resolve(json({
        printers: [{
          name: "kitchen",
          transport: "network",
          availability,
          profile: null,
          connection: { type: "network", host: "10.42.0.71", port: 9100 },
        }],
      }));
    }) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><FlashProbe /></AppDataProvider>);
    await act(async () => { await advanceTimers(0); });
    expect(screen.getByTestId("flashes").textContent).toBe("{}");

    await act(async () => { await advanceTimers(10_000); });
    expect(screen.getByTestId("flashes").textContent).toBe("{\"kitchen\":\"lost\"}");

    await act(async () => { await advanceTimers(10_000); });
    expect(screen.getByTestId("flashes").textContent).toBe("{}");
  });

  test("startScan closes the previous scan stream before opening the next one", async () => {
    FakeEventSource.instances = [];
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    globalThis.fetch = (() => Promise.resolve(json(printerInventory))) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><ScanProbe /></AppDataProvider>);
    const button = screen.getByRole("button", { name: "Scan" });

    await act(async () => { fireEvent.click(button); });
    expect(FakeEventSource.instances).toHaveLength(1);
    expect(FakeEventSource.instances[0]?.closed).toBe(false);

    await act(async () => { fireEvent.click(button); });
    expect(FakeEventSource.instances).toHaveLength(2);

    const [first, second] = FakeEventSource.instances;
    expect(first?.closed).toBe(true);
    // The property that matters is ordering, not just eventual closure: the
    // first stream must be closed before the second is even constructed, or
    // the previous scan keeps running to completion on the server (closing
    // the EventSource is the only cancellation mechanism it has).
    expect(first?.closedAt).not.toBeNull();
    expect(first?.closedAt as number).toBeLessThan(second?.constructedAt as number);
  });

  // A USB failure is tolerated rather than fatal, so a scan can report
  // several — one per device it could not open. They have to accumulate, and
  // in arrival order: the panel lists them positionally, because
  // `UsbEnumerationFailure` carries no bus or address and two of the same
  // model refused at two addresses are otherwise indistinguishable.
  test("usb_failure events accumulate in arrival order", async () => {
    FakeEventSource.instances = [];
    globalThis.EventSource = FakeEventSource as unknown as typeof EventSource;
    globalThis.fetch = (() => Promise.resolve(json(printerInventory))) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><ScanProbe /><ScanFailureProbe /></AppDataProvider>);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Scan" })); });
    const source = FakeEventSource.instances[0]!;

    const failure = (productId: number) => ({
      vendor_id: 0x0416,
      product_id: productId,
      stage: "open_device",
      reason: "permission denied (errno 13)",
      permission_denied: true,
      can_grant_usb_permissions: true,
    });

    await act(async () => { source.emit("usb_failure", failure(2)); });
    expect(screen.getByTestId("failures").textContent).toBe("2");

    await act(async () => { source.emit("usb_failure", failure(3)); });
    expect(screen.getByTestId("failures").textContent).toBe("2,3");
  });
});
