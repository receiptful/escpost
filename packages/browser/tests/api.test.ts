// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { escpost } from "../src/index";

describe("escpost.print()", () => {
  it("sends raw bytes as base64 and resolves with the job id", async () => {
    let seen: any = null;
    const stop = installFakeRelay((op, payload) => {
      seen = { op, payload };
      return { jobId: "job-1" };
    });

    const result = await escpost.print({ printer: "TM-T20", data: new Uint8Array([0x1b, 0x40, 0x41]) });

    expect(result).toEqual({ jobId: "job-1" });
    expect(seen.op).toBe("print");
    expect(seen.payload.printer).toBe("TM-T20");
    expect(seen.payload.data).toBe("G0BB"); // base64 of 1b 40 41
    stop();
  });

  it("accepts a plain string as raw data", async () => {
    let seen: any = null;
    const stop = installFakeRelay((_op, payload) => { seen = payload; return { jobId: "job-2" }; });
    await escpost.print({ printer: "TM-T20", data: "AB" });
    expect(seen.data).toBe("QUI=");
    stop();
  });

  it("forwards an HTML print to the worker and never prompts", async () => {
    let seen: any = null;
    const stop = installFakeRelay((op, payload) => {
      seen = { op, payload };
      return { jobId: "job-html" };
    });

    // No dialog, ever, mid-print: the answer comes from the worker.
    await escpost.print({ printer: "TM-T20", html: "<h1>Total</h1>" });

    expect(seen.op).toBe("print");
    expect(seen.payload.html).toBe("<h1>Total</h1>");
    stop();
  });

  it("rejects an unrecognised transport instead of printing locally", async () => {
    const stop = installFakeRelay(() => ({ jobId: "must-not-happen" }));
    await expect(escpost.print({ printer: "TM-T20", data: "x", target: "cloud" as any }))
      .rejects.toMatchObject({ code: "PRINT_FAILED" });
    stop();
  });

  it("rejects a call carrying neither data nor html", async () => {
    await expect(escpost.print({ printer: "TM-T20" } as any)).rejects.toMatchObject({ code: "PRINT_FAILED" });
  });

  it("lists printers", async () => {
    const stop = installFakeRelay(() => [{ id: "tm-t20", name: "TM-T20", transport: "usb", profile: null, status: "ready" }]);
    await expect(escpost.printers.list()).resolves.toHaveLength(1);
    stop();
  });
});

/**
 * happy-dom's window.postMessage sets event.source to something that is not
 * === window, which the transport correctly rejects. Deliver the reply the way a
 * real browser does instead.
 */
function postFromRelay(body: Record<string, unknown>) {
  window.dispatchEvent(new MessageEvent("message", { data: body, source: window as unknown as Window }));
}

function installFakeRelay(handler: (op: string, payload: any) => unknown) {
  const listener = (event: MessageEvent) => {
    const message = event.data;
    if (message?.source !== "escpost-page") return;
    postFromRelay({ source: "escpost-ext", id: message.id, ok: true, data: handler(message.op, message.payload) });
  };
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}
