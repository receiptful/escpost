// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { escpost } from "../src/index";

/**
 * These use real timers. happy-dom delivers window.postMessage on its own
 * schedule, which vitest's fake timers do not drive, so nothing that needs a
 * reply can be tested under them — only the timeout path can, because it needs
 * no reply at all.
 */
const POLL_MS = 250;

/** Torn down in afterEach, so a failing assertion cannot leave a relay
 *  listening and answering the next test. */
const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.useRealTimers();
});

describe("escpost.isAvailable()", () => {
  it("is true when the relay answers", async () => {
    installRelay(() => []);
    await expect(escpost.isAvailable()).resolves.toBe(true);
  });

  it("is false, rather than throwing, when nothing answers", async () => {
    vi.useFakeTimers();
    const pending = escpost.isAvailable();
    await vi.advanceTimersByTimeAsync(2000);
    await expect(pending).resolves.toBe(false);
  });

  it("is false when the daemon is down behind a working extension", async () => {
    installFailingRelay("DAEMON_NOT_RUNNING");
    await expect(escpost.isAvailable()).resolves.toBe(false);
  });
});

describe("escpost.printers.subscribe()", () => {
  it("reports the current list once, then only on change", async () => {
    let printers = [printer("tm-t20", "ready")];
    installRelay(() => printers);
    const seen: unknown[] = [];

    const unsubscribe = escpost.printers.subscribe((list) => seen.push(list), { intervalMs: POLL_MS });
    cleanups.push(unsubscribe);

    await waitFor(() => seen.length === 1);

    await sleep(POLL_MS * 2);
    expect(seen).toHaveLength(1);

    printers = [printer("tm-t20", "unavailable")];
    await waitFor(() => seen.length === 2);
    expect(seen[1]).toEqual([printer("tm-t20", "unavailable")]);
  });

  it("reports an empty list when the daemon stops answering", async () => {
    installFailingRelay("DAEMON_NOT_RUNNING");
    const seen: unknown[] = [];

    const unsubscribe = escpost.printers.subscribe((list) => seen.push(list), { intervalMs: POLL_MS });
    cleanups.push(unsubscribe);

    await waitFor(() => seen.length === 1);
    expect(seen[0]).toEqual([]);
  });

  it("stops polling once unsubscribed", async () => {
    let calls = 0;
    installRelay(() => {
      calls += 1;
      return [];
    });

    const unsubscribe = escpost.printers.subscribe(() => {}, { intervalMs: POLL_MS });
    await waitFor(() => calls > 0);
    unsubscribe();

    const afterUnsubscribe = calls;
    await sleep(POLL_MS * 3);
    expect(calls).toBe(afterUnsubscribe);
  });
});

function printer(id: string, status: "ready" | "unavailable") {
  return { id, name: id.toUpperCase(), transport: "usb", profile: null, status };
}

function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitFor(condition: () => boolean, timeoutMs = 3000) {
  const deadline = Date.now() + timeoutMs;
  while (!condition()) {
    if (Date.now() > deadline) throw new Error("condition never became true");
    await sleep(10);
  }
}

function installRelay(handler: () => unknown) {
  return install((message) => ({ source: "escpost-ext", id: message.id, ok: true, data: handler() }));
}

function installFailingRelay(code: string) {
  return install((message) => ({ source: "escpost-ext", id: message.id, ok: false, error: { code, message: code } }));
}

function install(reply: (message: { id: number }) => Record<string, unknown>) {
  const listener = (event: MessageEvent) => {
    const message = event.data;
    if (message?.source !== "escpost-page") return;
    window.dispatchEvent(new MessageEvent("message", { data: reply(message), source: window as unknown as Window }));
  };
  window.addEventListener("message", listener);
  const stop = () => window.removeEventListener("message", listener);
  cleanups.push(stop);
  return stop;
}
