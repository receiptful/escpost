// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createQzShim, installQzShim } from "../src/qz/surface";

const PRINTERS = [
  { id: "tm-t20", name: "TM-T20", transport: "usb", profile: "NT-5890K", status: "ready" },
  { id: "kitchen", name: "Kitchen", transport: "network", profile: null, status: "ready" },
];

let seen: Array<{ op: string; payload: any }> = [];

/** Stand in for the ISOLATED-world relay and the service worker behind it. */
/**
 * A reply must arrive as a real MessageEvent whose `source` IS `window`. happy-dom's
 * window.postMessage() sets `source` to a non-null object that is NOT === window, so a
 * reply sent that way is dropped by the transport's `event.source !== window` guard and
 * every call times out into EXTENSION_NOT_INSTALLED. That guard is a real security
 * property — without it any iframe could forge a printer list — so the test must be
 * browser-faithful rather than the guard weakened. Verified on happy-dom 20, 2026-08-20.
 */
function postFromRelay(body: Record<string, unknown>) {
  window.dispatchEvent(new MessageEvent("message", { data: body, source: window as unknown as Window }));
}

function installFakeRelay() {
  const listener = (event: MessageEvent) => {
    const message = event.data;
    if (message?.source !== "escpost-page") return;
    seen.push({ op: message.op, payload: message.payload });

    let data: unknown = null;
    if (message.op === "printers.list") data = PRINTERS;
    if (message.op === "printers.default") data = PRINTERS[0];
    if (message.op === "print") data = { jobId: "job-1" };

    postFromRelay({ source: "escpost-ext", id: message.id, ok: true, data });
  };
  window.addEventListener("message", listener);
  return () => window.removeEventListener("message", listener);
}

let stopRelay: () => void;

beforeEach(() => {
  seen = [];
  stopRelay = installFakeRelay();
});

afterEach(() => stopRelay());

describe("installQzShim", () => {
  it("installs window.qz when nothing else has", () => {
    const target = {} as Window & { qz?: unknown };
    expect(installQzShim(target)).toBe(true);
    expect((target.qz as { version: string }).version).toBe("2.2.4");
  });

  it("yields to a page that loads the real qz-tray.js (Q4)", () => {
    const target = { qz: { version: "real" } } as unknown as Window & { qz?: unknown };
    expect(installQzShim(target)).toBe(false);
    expect((target.qz as { version: string }).version).toBe("real");
  });

  it("leaves window.qz writable so a later qz-tray.js can take over", () => {
    const target = {} as Window & { qz?: unknown };
    installQzShim(target);
    target.qz = { version: "real" };
    expect((target.qz as { version: string }).version).toBe("real");
  });
});

describe("the shim's public API", () => {
  it("accepts every qz.security setter without prompting (Q1)", () => {
    const qz = createQzShim();
    expect(() => qz.security.setCertificatePromise((resolve: any) => resolve("cert"))).not.toThrow();
    expect(() => qz.security.setSignaturePromise(() => (resolve: any) => resolve(""))).not.toThrow();
    expect(() => qz.security.setSignatureAlgorithm("SHA512")).not.toThrow();
    expect(seen).toHaveLength(0);
  });

  it("connects, reports itself active, and disconnects", async () => {
    const qz = createQzShim();
    expect(qz.websocket.isActive()).toBe(false);
    await expect(qz.websocket.connect()).resolves.toBeUndefined();
    expect(qz.websocket.isActive()).toBe(true);
    await expect(qz.websocket.disconnect()).resolves.toBeUndefined();
    expect(qz.websocket.isActive()).toBe(false);
  });

  it("reports the version qz-tray.js applies no legacy rewrites for", async () => {
    const qz = createQzShim();
    await qz.websocket.connect();
    await expect(qz.api.getVersion()).resolves.toBe("2.2.4");
  });

  it("returns plain name strings from printers.find, as QZ does", async () => {
    const qz = createQzShim();
    await qz.websocket.connect();
    await expect(qz.printers.find()).resolves.toEqual(["TM-T20", "Kitchen"]);
  });

  it("returns the single matched name when printers.find is given a query", async () => {
    const qz = createQzShim();
    await qz.websocket.connect();
    await expect(qz.printers.find("kitchen")).resolves.toBe("Kitchen");
  });

  it("names the printers it does know when a query matches nothing (N1)", async () => {
    const qz = createQzShim();
    await qz.websocket.connect();
    await expect(qz.printers.find("Star TSP100")).rejects.toThrow(/TM-T20, Kitchen/);
  });

  it("returns the default printer's name", async () => {
    const qz = createQzShim();
    await qz.websocket.connect();
    await expect(qz.printers.getDefault()).resolves.toBe("TM-T20");
  });

  it("prints raw bytes as base64 under the resolved escpost printer id", async () => {
    const qz = createQzShim();
    await qz.websocket.connect();
    // qz.print resolves with the call's result, which is null (qz-tray.js:1688-1694).
    await expect(qz.print(qz.configs.create("TM-T20"), ["\x1b@hello"])).resolves.toBeNull();
    const print = seen.find((entry) => entry.op === "print");
    expect(print!.payload).toEqual({ printer: "tm-t20", data: "G0BoZWxsbw==" });
  });

  it("forwards an unresolved name verbatim so the worker can apply the user's aliases (N2)", async () => {
    const qz = createQzShim();
    await qz.websocket.connect();
    await qz.print(qz.configs.create("EPSON TM-T20II"), ["\x1b@"]);
    const print = seen.find((entry) => entry.op === "print");
    expect(print!.payload.printer).toBe("EPSON TM-T20II");
  });

  it("rejects an HTML job and never reaches the worker (Q3)", async () => {
    const qz = createQzShim();
    await qz.websocket.connect();
    await expect(qz.print(qz.configs.create("TM-T20"), [{ type: "pixel", format: "html", data: "<h1>x</h1>" }]))
      .rejects.toThrow(/@escpost\/browser/);
    expect(seen.some((entry) => entry.op === "print")).toBe(false);
  });

  it("rejects a print made before connect(), exactly as qz-tray.js does", async () => {
    const qz = createQzShim();
    await expect(qz.print(qz.configs.create("TM-T20"), ["\x1b@"]))
      .rejects.toThrow(/connection to QZ has not been established/);
  });

  it("gives a config all of qz-tray.js's default options, not just the set ones", () => {
    const qz = createQzShim();
    const config = qz.configs.create("TM-T20", { copies: 2 });
    const options = config.getOptions();
    expect(options.copies).toBe(2);
    expect(options.units).toBe("in");
    expect(options.colorType).toBe("color");
    expect("spool" in options).toBe(true);
  });
});
