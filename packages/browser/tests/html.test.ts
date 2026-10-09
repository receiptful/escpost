// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { escpost } from "../src/index";

describe("escpost.print({ html })", () => {
  it("forwards the html to the worker rather than answering locally", async () => {
    let seen: any = null;
    const stop = installFakeRelay((op, payload) => {
      seen = { op, payload };
      return { jobId: "job-9" };
    });

    const result = await escpost.print({ printer: "TM-T20", html: "<h1>Total</h1>" });

    expect(result).toEqual({ jobId: "job-9" });
    expect(seen.op).toBe("print");
    expect(seen.payload).toEqual({ printer: "TM-T20", html: "<h1>Total</h1>" });
    // The package contains no renderer. It posts a message; that is all.
    expect(seen.payload.data).toBeUndefined();
    stop();
  });

  it("surfaces NOT_SIGNED_IN from the worker without prompting", async () => {
    const stop = installFailingRelay("NOT_SIGNED_IN", "HTML receipts need an account.");

    await expect(escpost.print({ printer: "TM-T20", html: "<h1>x</h1>" })).rejects.toMatchObject({
      code: "NOT_SIGNED_IN",
    });
    stop();
  });

  it("surfaces QUOTA_EXCEEDED and says raw is unaffected", async () => {
    const stop = installFailingRelay("QUOTA_EXCEEDED", "All used up.");

    const error = await escpost
      .print({ printer: "TM-T20", html: "<h1>x</h1>" })
      .catch((caught) => caught);

    expect(error.code).toBe("QUOTA_EXCEEDED");
    expect(error.message).toContain("Raw printing is unaffected.");
    stop();
  });

  it("still rejects a job carrying neither data nor html", async () => {
    const stop = installFakeRelay(() => ({ jobId: "must-not-happen" }));
    await expect(escpost.print({ printer: "TM-T20" } as any)).rejects.toMatchObject({
      code: "PRINT_FAILED",
    });
    stop();
  });
});

/**
 * happy-dom's window.postMessage sets event.source to something that is not
 * === window, which the transport correctly rejects. Deliver the reply the way
 * a real browser does instead. Same helper as api.test.ts, deliberately
 * duplicated rather than shared: these files test different guarantees and
 * neither should be able to break the other's harness.
 */
function installFakeRelay(handler: (op: string, payload: any) => unknown) {
  const listener = (event: MessageEvent) => {
    const message = event.data;
    if (message?.source !== "escpost-page") return;
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { source: "escpost-ext", id: message.id, ok: true, data: handler(message.op, message.payload) },
        source: window as unknown as Window,
      }),
    );
  };
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}

function installFailingRelay(code: string, message: string) {
  const listener = (event: MessageEvent) => {
    const incoming = event.data;
    if (incoming?.source !== "escpost-page") return;
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { source: "escpost-ext", id: incoming.id, ok: false, error: { code, message } },
        source: window as unknown as Window,
      }),
    );
  };
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}
