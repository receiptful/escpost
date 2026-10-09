// @vitest-environment happy-dom
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";

const PRINTERS = [
  { id: "tm-t20", name: "TM-T20", transport: "usb", profile: "NT-5890K", status: "ready" },
  { id: "kitchen", name: "Kitchen", transport: "network", profile: null, status: "ready" },
];

/**
 * qz-tray.js is a UMD bundle. Its footer prefers AMD, then CommonJS, and only
 * assigns `window.qz` when neither exists (qz-tray.js:2849-2857). A Vite-transformed
 * `import` of the file could satisfy the CommonJS branch and never touch the window,
 * so evaluate the real source instead and pass `define`, `exports` and `module` in as
 * undefined, which forces the browser branch. `window` is the happy-dom global, which
 * is also where `WebSocket` resolves from when qz-tray.js:681 captures it.
 */
function loadQzTray(): any {
  // NOT `new URL(..., import.meta.url)`: under happy-dom the global URL ignores the
  // base argument and resolves against the document location, yielding
  // http://localhost:3000/... , which fileURLToPath then rightly refuses.
  const source = readFileSync(join(import.meta.dirname, "..", "vendor", "qz-tray.js"), "utf8");
  const evaluate = new Function("window", "define", "exports", "module", source);
  evaluate(globalThis, undefined, undefined, undefined);
  return (globalThis as any).qz;
}

/** A real socket class, so the passthrough assertion does not depend on happy-dom. */
class StubNative {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  constructor(public url: string) {}
  send(): void {}
  close(): void {}
}

let qz: any;
let sockets: any[] = [];
let seen: Array<{ op: string; payload: any }> = [];
let stopRelay: () => void;

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
    const message = (event as any).data;
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

beforeAll(async () => {
  // 1. A native to fall back to, before anything captures it.
  (globalThis as any).WebSocket = StubNative;

  // 2. The patch, which captures that native and replaces the global.
  await import("../src/ws-patch");

  // 3. A recording wrapper, so the tests can reach the socket qz-tray.js opens.
  //    A Proxy forwards the static reads (CONNECTING/OPEN/CLOSING/CLOSED) untouched.
  const patched = (globalThis as any).WebSocket;
  (globalThis as any).WebSocket = new Proxy(patched, {
    construct(target, args: any[]) {
      const socket = new target(...args);
      sockets.push(socket);
      return socket;
    },
  });

  // 4. Only now the real client, which captures WebSocket at evaluation time.
  qz = loadQzTray();
  // Pinned on purpose. These tests describe one client's behaviour, so a
  // vendored upgrade has to fail here and be read, rather than pass quietly
  // against different code.
  expect(qz.version).toBe("2.2.6");
});

beforeEach(async () => {
  seen = [];
  sockets = [];
  stopRelay = installFakeRelay();
  await qz.websocket.connect();
});

afterEach(async () => {
  if (qz.websocket.isActive()) await qz.websocket.disconnect();
  stopRelay();
});

afterAll(() => {
  delete (globalThis as any).qz;
});

describe("the real qz-tray.js 2.2.6 against the escpost patch", () => {
  it("connects, which means the certificate handshake was answered (W2)", () => {
    expect(qz.websocket.isActive()).toBe(true);
  });

  it("reports a version the client parsed as semver", async () => {
    // What escpost reports, not the client's own version: 2.2.4 is the
    // threshold of the last version-gated payload rewrite, so reporting it
    // keeps 2.2.6 sending data verbatim.
    await expect(qz.api.getVersion()).resolves.toBe("2.2.4");
  });

  it("describes the connection it thinks it has", () => {
    const info = qz.websocket.getConnectionInfo();
    expect(info.host).toBe("localhost");
    expect([8181, 8282, 8383, 8484, 8182, 8283, 8384, 8485]).toContain(info.port);
  });

  it("finds the printers the daemon reported, by name", async () => {
    await expect(qz.printers.find()).resolves.toEqual(["TM-T20", "Kitchen"]);
  });

  it("finds the default printer", async () => {
    await expect(qz.printers.getDefault()).resolves.toBe("TM-T20");
  });

  it("prints a raw job, and the worker receives the right bytes", async () => {
    // qz.print resolves with the call's result (qz-tray.js:1688-1694), which is null.
    await expect(qz.print(qz.configs.create("TM-T20"), ["\x1b@hello"])).resolves.toBeNull();
    expect(seen.find((entry) => entry.op === "print")!.payload).toEqual({
      printer: "tm-t20",
      data: "G0BoZWxsbw==",
    });
  });

  it("rejects a pixel/html job with an error naming the supported path (Q3)", async () => {
    await expect(
      qz.print(qz.configs.create("TM-T20"), [{ type: "pixel", format: "html", flavor: "plain", data: "<h1>Total</h1>" }]),
    ).rejects.toThrow(/@escpost\/browser/);
    expect(seen.some((entry) => entry.op === "print")).toBe(false);
  });

  it("takes every qz.security setter without prompting, and signs with an empty signature (Q1)", async () => {
    expect(() => qz.security.setCertificatePromise((resolve: any) => resolve("no-cert-needed"))).not.toThrow();
    expect(() => qz.security.setSignaturePromise(() => (resolve: any) => resolve(""))).not.toThrow();
    expect(() => qz.security.setSignatureAlgorithm("SHA512")).not.toThrow();
    await expect(qz.printers.find()).resolves.toHaveLength(2);
  });

  it("swallows the keep-alive ping and sends nothing back", async () => {
    const socket = sockets.at(-1);
    const replies: string[] = [];
    const original = socket.onmessage;
    socket.onmessage = function (event: { data: string }) {
      replies.push(event.data);
      original.call(this, event);
    };

    socket.send("ping");
    await new Promise((resolve) => setTimeout(resolve, 20));

    expect(replies).toEqual([]);
    socket.onmessage = original;
  });

  it("gets an error back from an unimplemented call rather than hanging (W3)", async () => {
    await expect(qz.websocket.getNetworkInfo()).rejects.toThrow(/networking\.device/);
  });

  it("stays connected after that error, because the reply carried a uid (W3)", async () => {
    await qz.websocket.getNetworkInfo().catch(() => {});
    expect(qz.websocket.isActive()).toBe(true);
    await expect(qz.printers.find()).resolves.toHaveLength(2);
  });

  it("disconnects, which needs `this` bound to the socket inside onclose", async () => {
    await expect(qz.websocket.disconnect()).resolves.toBeUndefined();
    expect(qz.websocket.isActive()).toBe(false);
  });

  it("gives a non-QZ URL a real socket, not the shim", () => {
    const socket = new (globalThis as any).WebSocket("wss://chat.example.com/socket");
    expect(socket).toBeInstanceOf(StubNative);
    expect(socket.url).toBe("wss://chat.example.com/socket");
  });
});
