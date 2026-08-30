// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

/** A page message must look like one a browser delivers: source === window. */
function postFromPage(body: Record<string, unknown>) {
  window.dispatchEvent(new MessageEvent("message", { data: body, source: window as unknown as Window }));
}

function nextReply(): Promise<any> {
  return new Promise((resolve) => {
    const listener = (event: MessageEvent) => {
      if ((event.data as any)?.source === "escpost-ext") {
        window.removeEventListener("message", listener);
        resolve(event.data);
      }
    };
    window.addEventListener("message", listener);
  });
}

let sendMessage: ReturnType<typeof vi.fn>;

beforeEach(async () => {
  sendMessage = vi.fn((_msg: unknown, callback: (response: unknown) => void) => callback({ ok: true, data: ["TM-T20"] }));
  (globalThis as any).chrome = { runtime: { sendMessage, lastError: undefined } };
  vi.resetModules();
  await import("../src/relay");           // registers the listener
});

afterEach(() => {
  delete (globalThis as any).chrome;
});

describe("relay", () => {
  it("forwards a page message to the worker and echoes the reply with the same id", async () => {
    const reply = nextReply();
    postFromPage({ source: "escpost-page", id: 7, protocol: 1, op: "printers.list", payload: undefined });
    await expect(reply).resolves.toMatchObject({ source: "escpost-ext", id: 7, ok: true, data: ["TM-T20"] });
    expect(sendMessage).toHaveBeenCalledWith(
      expect.objectContaining({ op: "printers.list" }),
      expect.any(Function),
    );
  });

  it("ignores a message that is not ours, so it cannot be driven by another library", () => {
    postFromPage({ source: "some-other-library", id: 1, op: "print", payload: {} });
    expect(sendMessage).not.toHaveBeenCalled();
  });

  it("ignores a message with no numeric id, which could not be answered anyway", () => {
    postFromPage({ source: "escpost-page", op: "printers.list" });
    expect(sendMessage).not.toHaveBeenCalled();
  });

  it("turns a dead worker into a typed error rather than silence", async () => {
    sendMessage.mockImplementation((_msg: unknown, callback: (r: unknown) => void) => {
      (globalThis as any).chrome.runtime.lastError = { message: "Could not establish connection." };
      callback(undefined);
    });
    const reply = nextReply();
    postFromPage({ source: "escpost-page", id: 9, op: "printers.list", payload: undefined });
    // Silence here would hang the page's promise until its 2s timeout and then
    // lie about the cause, blaming a missing extension for a worker that died.
    // Assert the worker's OWN message, not just the code: without this the test
    // passes via the `response ?? fallback` branch below and the lastError
    // handling could be deleted entirely without failing anything.
    await expect(reply).resolves.toMatchObject({
      id: 9,
      ok: false,
      error: { code: "PRINT_FAILED", message: "Could not establish connection." },
    });
  });

  it("answers even when the worker responds with nothing at all", async () => {
    sendMessage.mockImplementation((_msg: unknown, callback: (r: unknown) => void) => callback(undefined));
    const reply = nextReply();
    postFromPage({ source: "escpost-page", id: 11, op: "printers.list", payload: undefined });
    // The other half of the pair: no lastError, no response — the generic fallback.
    await expect(reply).resolves.toMatchObject({
      id: 11,
      ok: false,
      error: { code: "PRINT_FAILED", message: "The extension did not respond." },
    });
  });
});
