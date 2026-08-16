import { afterEach, describe, expect, jest, test } from "bun:test";
import { act, cleanup, render, screen } from "@testing-library/preact";
import { AppDataProvider, useAppData } from "./data";

const readyStatus = { virtual_printer: null, jobs_processed: 3 };
const printerInventory = { printers: [] };

function json(body: unknown) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

function Probe() {
  const { connection, printers } = useAppData();
  return <p>{`${connection}:${printers.phase}`}</p>;
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
      return Promise.resolve(json(printerInventory));
    });
    globalThis.fetch = fetch as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><Probe /></AppDataProvider>);
    expect(fetch).toHaveBeenCalledTimes(2);

    await act(async () => {
      resolveFirst(json(readyStatus));
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByText("ready:ready")).toBeTruthy();
    await act(async () => {
      jest.advanceTimersByTime(1_999);
    });
    expect(fetch).toHaveBeenCalledTimes(2);

    await act(async () => {
      jest.advanceTimersByTime(1);
    });
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  test("aborts the active status request when unmounted", () => {
    let signal: AbortSignal | undefined;
    globalThis.fetch = jest.fn((input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input) === "/api/status") {
        signal = init?.signal ?? undefined;
        return new Promise<Response>(() => {});
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
      printerRequests += 1;
      return Promise.resolve(json(printerInventory));
    }) as unknown as typeof globalThis.fetch;

    render(<AppDataProvider><Probe /></AppDataProvider>);
    expect(await screen.findByText("disconnected:ready")).toBeTruthy();
    expect(printerRequests).toBe(1);

    await act(async () => {
      jest.advanceTimersByTime(2_000);
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByText("ready:ready")).toBeTruthy();
    expect(printerRequests).toBe(2);
  });
});
