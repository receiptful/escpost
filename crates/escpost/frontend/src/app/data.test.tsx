import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, render, screen } from "@testing-library/preact";
import { AppDataProvider, useAppData } from "./data";

const readyStatus = { virtual_printer: null, jobs_processed: 3 };
const printerInventory = { printers: [] };

function json(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function Probe() {
  const { connection, printers, statusError } = useAppData();
  return <p>{`${connection}:${printers.phase}:${statusError?.message ?? "none"}`}</p>;
}

afterEach(() => {
  cleanup();
  jest.useRealTimers();
});

describe("AppDataProvider", () => {
  test("waits two seconds after a status response before starting the next request", async () => {
    jest.useFakeTimers();
    let resolveFirst!: (response: Response) => void;
    const fetch = jest.fn((input: RequestInfo | URL) => {
      if (String(input) === "/api/status") {
        return new Promise<Response>((resolve) => {
          resolveFirst = resolve;
        });
      }
      if (String(input) === "/api/profiles/list") {
        return Promise.resolve(json({ profiles: [] }));
      }
      return Promise.resolve(json(printerInventory));
    });
    globalThis.fetch = fetch as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><Probe /></AppDataProvider>);
    expect(fetch).toHaveBeenCalledTimes(3);

    await act(async () => {
      resolveFirst(json(readyStatus));
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByText("ready:ready:none")).toBeTruthy();
    await act(async () => {
      jest.advanceTimersByTime(1_999);
    });
    expect(fetch).toHaveBeenCalledTimes(3);

    await act(async () => {
      jest.advanceTimersByTime(1);
    });
    expect(fetch).toHaveBeenCalledTimes(4);
  });

  test("aborts the active status request when unmounted", () => {
    let signal: AbortSignal | undefined;
    globalThis.fetch = jest.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input) === "/api/status") {
        signal = init?.signal ?? undefined;
        return new Promise<Response>(() => {});
      }
      if (String(input) === "/api/profiles/list") {
        return Promise.resolve(json({ profiles: [] }));
      }
      return Promise.resolve(json(printerInventory));
    }) as unknown as typeof globalThis.fetch;

    const view = render(<AppDataProvider><Probe /></AppDataProvider>);
    view.unmount();
    expect(signal?.aborted).toBe(true);
  });

  test("recovers from a status network failure and refreshes printers once", async () => {
    jest.useFakeTimers();
    const statusResults = [() => Promise.reject(new TypeError("offline")), () => Promise.resolve(json(readyStatus))];
    let printerRequests = 0;
    globalThis.fetch = jest.fn((input: RequestInfo | URL) => {
      if (String(input) === "/api/status") {
        return statusResults.shift()!();
      }
      if (String(input) === "/api/profiles/list") {
        return Promise.resolve(json({ profiles: [] }));
      }
      printerRequests += 1;
      return Promise.resolve(json(printerInventory));
    }) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><Probe /></AppDataProvider>);
    expect(await screen.findByText("disconnected:ready:Unable to reach the ESCPost server.")).toBeTruthy();
    expect(printerRequests).toBe(1);

    await act(async () => {
      jest.advanceTimersByTime(2_000);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByText("ready:ready:none")).toBeTruthy();
    expect(printerRequests).toBe(2);
  });

  test("keeps a ready connection when the status endpoint returns an API error", async () => {
    jest.useFakeTimers();
    const statusResults = [
      () => Promise.resolve(json(readyStatus)),
      () => Promise.resolve(json({ error: { code: "status_unavailable", message: "Status is unavailable." } }, 503)),
    ];
    globalThis.fetch = jest.fn((input: RequestInfo | URL) => String(input) === "/api/status"
      ? statusResults.shift()!()
      : String(input) === "/api/profiles/list"
        ? Promise.resolve(json({ profiles: [] }))
      : Promise.resolve(json(printerInventory))) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><Probe /></AppDataProvider>);
    expect(await screen.findByText("ready:ready:none")).toBeTruthy();

    await act(async () => {
      jest.advanceTimersByTime(2_000);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByText("ready:ready:Status is unavailable.")).toBeTruthy();
  });
});
