import { afterEach, expect, jest, test } from "bun:test";
import { act, cleanup, render, screen } from "@testing-library/preact";
import { useCurrentJob } from "./use-current-job";

function json(body: unknown) {
  return new Response(JSON.stringify(body), { headers: { "content-type": "application/json" } });
}

function response(id: string | null) {
  return {
    receiving: false,
    profile: "REFERENCE",
    error: null,
    job: id === null ? null : { id, antialias: false, warnings: [], sheets: [] },
  };
}

function Probe() {
  const resource = useCurrentJob();
  return <p>{`${resource.data?.job?.id ?? "none"}:${resource.error?.message ?? "ok"}:${resource.loading}`}</p>;
}

afterEach(() => {
  cleanup();
  jest.useRealTimers();
});

test("retains the last job while unavailable and clears it after a successful empty response", async () => {
  jest.useFakeTimers();
  const results = [
    () => Promise.resolve(json(response("7"))),
    () => Promise.reject(new TypeError("offline")),
    () => Promise.resolve(json(response(null))),
  ];
  globalThis.fetch = jest.fn(() => results.shift()!()) as unknown as typeof fetch;
  render(<Probe />);

  expect(await screen.findByText("7:ok:false")).toBeTruthy();
  await act(async () => {
    jest.advanceTimersByTime(750);
    await Promise.resolve();
    await Promise.resolve();
  });
  expect(screen.getByText("7:Unable to reach the ESCPost server.:false")).toBeTruthy();

  await act(async () => {
    jest.advanceTimersByTime(750);
    await Promise.resolve();
    await Promise.resolve();
  });
  expect(screen.getByText("none:ok:false")).toBeTruthy();
});

test("aborts the active current-job request when the page unmounts", () => {
  let signal: AbortSignal | undefined;
  globalThis.fetch = jest.fn((_: RequestInfo | URL, init?: RequestInit) => {
    signal = init?.signal ?? undefined;
    return new Promise<Response>(() => {});
  }) as unknown as typeof fetch;

  const view = render(<Probe />);
  view.unmount();
  expect(signal?.aborted).toBe(true);
});
