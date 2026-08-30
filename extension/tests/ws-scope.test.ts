// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { createPatchedWebSocket, QZ_PORTS } from "../src/ws-patch";

/**
 * Security finding 4. The patch decided what to intercept from the port alone,
 * so a page connecting to its own server on wss://relay.example.com:8181 — a
 * perfectly ordinary port for a WebSocket service — had that connection
 * silently taken over by this extension and answered with QZ protocol frames.
 * The real QZ Tray only ever listens on the local machine.
 */
function nativeSpy() {
  const made: string[] = [];
  class FakeNative {
    static readonly CONNECTING = 0;
    static readonly OPEN = 1;
    static readonly CLOSING = 2;
    static readonly CLOSED = 3;
    constructor(url: string) {
      made.push(url);
    }
    addEventListener() {}
    removeEventListener() {}
    send() {}
    close() {}
  }
  return { made, Native: FakeNative as unknown as typeof WebSocket };
}

function patched() {
  const { made, Native } = nativeSpy();
  const send = vi.fn(async () => []) as unknown as Parameters<typeof createPatchedWebSocket>[1];
  return { made, Patched: createPatchedWebSocket(Native, send) };
}

describe("the WebSocket patch only intercepts the local machine", () => {
  it.each(["localhost", "127.0.0.1"])("intercepts QZ on %s", (host) => {
    const { made, Patched } = patched();
    new Patched(`wss://${host}:8181`);
    expect(made).toEqual([]); // intercepted: no native socket was opened
  });

  it("passes a remote host on a QZ port straight through to the real WebSocket", () => {
    const { made, Patched } = patched();
    new Patched("wss://relay.example.com:8181");
    expect(made).toEqual(["wss://relay.example.com:8181"]);
  });

  it("passes every QZ port through when the host is not local", () => {
    for (const port of QZ_PORTS) {
      const { made, Patched } = patched();
      new Patched(`wss://someserver.test:${port}`);
      expect(made, `port ${port}`).toEqual([`wss://someserver.test:${port}`]);
    }
  });

  it("still passes a non-QZ port on localhost through", () => {
    const { made, Patched } = patched();
    new Patched("ws://localhost:3000");
    expect(made).toEqual(["ws://localhost:3000"]);
  });
});
