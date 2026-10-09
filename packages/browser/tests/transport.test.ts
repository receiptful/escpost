// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { request } from "../src/transport";

describe("request()", () => {
  beforeEach(() => vi.useRealTimers());

  it("resolves with the relay's data", async () => {
    const stop = installFakeRelay(() => [{ id: "tm-t20", name: "TM-T20" }]);
    await expect(request("printers.list", undefined)).resolves.toEqual([{ id: "tm-t20", name: "TM-T20" }]);
    stop();
  });

  it("rejects with EXTENSION_NOT_INSTALLED when nothing answers", async () => {
    vi.useFakeTimers();
    const pending = request("printers.list", undefined, { timeoutMs: 2000 });
    const assertion = expect(pending).rejects.toMatchObject({ code: "EXTENSION_NOT_INSTALLED" });
    await vi.advanceTimersByTimeAsync(2000);
    await assertion;
  });

  it("names the install path in the EXTENSION_NOT_INSTALLED message", async () => {
    vi.useFakeTimers();
    const pending = request("printers.list", undefined, { timeoutMs: 2000 });
    const assertion = expect(pending).rejects.toThrow(/chrome\.google\.com\/webstore|Chrome Web Store/);
    await vi.advanceTimersByTimeAsync(2000);
    await assertion;
  });

  it("does not confuse two concurrent calls", async () => {
    const stop = installFakeRelay((op, payload) => ({ op, payload }));
    const [first, second] = await Promise.all([request("a", 1), request("b", 2)]);
    expect(first).toEqual({ op: "a", payload: 1 });
    expect(second).toEqual({ op: "b", payload: 2 });
    stop();
  });

  it("surfaces a relay error as an EscpostError with its code intact", async () => {
    const stop = installFakeRelay(() => { throw new Error("boom"); });
    await expect(request("printers.list", undefined)).rejects.toMatchObject({ code: "PRINT_FAILED" });
    stop();
  });

  it("ignores messages that are not ours", async () => {
    vi.useFakeTimers();
    postFromRelay({ source: "some-other-library", id: 1, ok: true, data: "wrong" });
    const pending = request("printers.list", undefined, { timeoutMs: 2000 });
    const assertion = expect(pending).rejects.toMatchObject({ code: "EXTENSION_NOT_INSTALLED" });
    await vi.advanceTimersByTimeAsync(2000);
    await assertion;
  });
  it("reports a long-running call as a print failure, not a missing extension", async () => {
    vi.useFakeTimers();
    const pending = request("print", undefined, { timeoutMs: 20_000 });
    const assertion = expect(pending).rejects.toMatchObject({ code: "PRINT_FAILED" });
    await vi.advanceTimersByTimeAsync(20_000);
    await assertion;
  });

  it("does not resolve a reply addressed to another copy of this package", async () => {
    vi.useFakeTimers();
    const pending = request("printers.list", undefined, { timeoutMs: 2000 });
    // Another copy counts from its own block, so low ids are never ours.
    postFromRelay({ source: "escpost-ext", id: 1, ok: true, data: "not yours" });
    const assertion = expect(pending).rejects.toMatchObject({ code: "EXTENSION_NOT_INSTALLED" });
    await vi.advanceTimersByTimeAsync(2000);
    await assertion;
  });
});

/**
 * Deliver a reply the way a real browser does. happy-dom's window.postMessage sets
 * event.source to something that is not === window, which the transport correctly
 * rejects, so construct the MessageEvent explicitly instead.
 */
function postFromRelay(body: Record<string, unknown>) {
  window.dispatchEvent(new MessageEvent("message", { data: body, source: window as unknown as Window }));
}

/** Stand in for the ISOLATED-world content script: answer every request that arrives. */
function installFakeRelay(handler: (op: string, payload: unknown) => unknown) {
  const listener = (event: MessageEvent) => {
    const message = event.data;
    if (message?.source !== "escpost-page") return;
    let reply: Record<string, unknown>;
    try {
      reply = { source: "escpost-ext", id: message.id, ok: true, data: handler(message.op, message.payload) };
    } catch (error) {
      reply = { source: "escpost-ext", id: message.id, ok: false, error: { code: "PRINT_FAILED", message: String(error) } };
    }
    postFromRelay(reply);
  };
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}
