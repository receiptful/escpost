// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { createPatchedWebSocket, QZ_PORTS } from "../src/ws-patch";

const PRINTERS = [
  { id: "tm-t20", name: "TM-T20", transport: "usb", profile: "NT-5890K", status: "ready" },
  { id: "kitchen", name: "Kitchen", transport: "network", profile: null, status: "ready" },
];

class StubNative {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  constructor(public url: string) {}
  send(): void {}
  close(): void {}
}

/** Records what the socket asked the worker for, and answers like the worker would. */
function makeRequester() {
  const calls: Array<{ op: string; payload: any }> = [];
  const send = async <T>(op: string, payload: unknown): Promise<T> => {
    calls.push({ op, payload });
    if (op === "printers.list") return PRINTERS as T;
    if (op === "printers.default") return PRINTERS[0] as T;
    if (op === "print") return { jobId: "job-1" } as T;
    throw new Error(`unexpected op ${op}`);
  };
  return { calls, send };
}

/** Open a socket, collect every outbound message, and drive it like qz-tray.js would. */
function openSocket(send: ReturnType<typeof makeRequester>["send"]) {
  const Patched = createPatchedWebSocket(StubNative as unknown as typeof WebSocket, send);
  const socket: any = new Patched("ws://localhost:8182");
  const received: any[] = [];
  socket.onmessage = function (event: { data: string }) {
    received.push(JSON.parse(event.data));
  };
  const opened = new Promise<void>((resolve) => {
    socket.onopen = () => resolve();
  });
  return { socket, received, opened };
}

/** Wait until the socket has answered, without guessing at a timeout. */
async function nextMessage(received: any[]): Promise<any> {
  for (let attempt = 0; attempt < 50 && received.length === 0; attempt++) {
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  return received.shift();
}

describe("the patched constructor", () => {
  it("exposes the statics qz-tray.js:1211 refuses to connect without", () => {
    const Patched = createPatchedWebSocket(StubNative as unknown as typeof WebSocket, makeRequester().send);
    expect(Patched.CONNECTING).toBe(0);
    expect(Patched.OPEN).toBe(1);
    expect(Patched.CLOSING).toBe(2);
    expect(Patched.CLOSED).toBe(3);
  });

  it("intercepts all eight ports QZ tries", () => {
    const Patched = createPatchedWebSocket(StubNative as unknown as typeof WebSocket, makeRequester().send);
    expect(QZ_PORTS).toEqual([8181, 8282, 8383, 8484, 8182, 8283, 8384, 8485]);
    for (const port of QZ_PORTS) {
      expect(new Patched(`wss://localhost:${port}`)).not.toBeInstanceOf(StubNative);
    }
  });

  it("hands every other socket back to the real WebSocket (W3)", () => {
    const Patched = createPatchedWebSocket(StubNative as unknown as typeof WebSocket, makeRequester().send);
    expect(new Patched("wss://chat.example.com/socket")).toBeInstanceOf(StubNative);
    expect(new Patched("ws://localhost:3000/hmr")).toBeInstanceOf(StubNative);
    expect(new Patched("https://not-a-socket.example.com")).toBeInstanceOf(StubNative);
  });

  it("opens asynchronously and moves readyState CONNECTING -> OPEN", async () => {
    const { socket, opened } = openSocket(makeRequester().send);
    expect(socket.readyState).toBe(0);
    await opened;
    expect(socket.readyState).toBe(1);
  });

  it("tolerates the properties qz-tray.js writes straight onto the socket", async () => {
    const { socket, opened } = openSocket(makeRequester().send);
    await opened;
    socket.established = true;
    socket.interval = 7;
    socket.version = "2.2.4";
    socket.semver = [2, 2, 4, 0];
    socket.promise = { resolve() {}, reject() {} };
    expect(socket.established).toBe(true);
    // getConnectionInfo() splits .url on /[:\/]+/ and expects scheme://host:port.
    expect(socket.url.split(/[:/]+/g)).toEqual(["ws", "localhost", "8182"]);
  });
});

describe("the message protocol", () => {
  it("resolves the handshake message that has no call key (W2)", async () => {
    const { socket, received, opened } = openSocket(makeRequester().send);
    await opened;
    socket.send(JSON.stringify({ certificate: null, timestamp: 1, uid: "def456", position: { x: 0, y: 0 } }));
    const reply = await nextMessage(received);
    expect(reply.uid).toBe("def456");
    expect(reply.error).toBeUndefined();
    expect(reply.result).toBeDefined();
  });

  it("answers getVersion with a version qz-tray.js can parse as semver", async () => {
    const { socket, received, opened } = openSocket(makeRequester().send);
    await opened;
    socket.send(JSON.stringify({ call: "getVersion", params: null, timestamp: 1, uid: "abc123" }));
    expect(await nextMessage(received)).toEqual({ uid: "abc123", result: "2.2.4" });
  });

  it("carries a uid on every reply, because a bare one closes the connection (W3)", async () => {
    const { socket, received, opened } = openSocket(makeRequester().send);
    await opened;
    socket.send(JSON.stringify({ call: "getVersion", uid: "u1" }));
    socket.send(JSON.stringify({ call: "printers.find", params: {}, uid: "u2" }));
    socket.send(JSON.stringify({ call: "nope.at.all", params: {}, uid: "u3" }));

    // Replies are NOT ordered: getVersion answers synchronously, the unknown-call
    // error settles on a microtask, and printers.find awaits the worker. A real QZ
    // Tray answers out of order too, which is exactly why qz-tray.js correlates by
    // uid alone (qz-tray.js:333-345). Assert the set, never the sequence.
    for (let attempt = 0; attempt < 50 && received.length < 3; attempt++) {
      await new Promise((resolve) => setTimeout(resolve, 1));
    }
    expect(received).toHaveLength(3);
    for (const reply of received) {
      expect(reply.uid).not.toBeNull();
      expect(reply.uid).not.toBeUndefined();
    }
    expect(received.map((reply: any) => reply.uid).sort()).toEqual(["u1", "u2", "u3"]);
  });

  it("swallows the keep-alive ping and answers nothing at all", async () => {
    const { socket, received, opened } = openSocket(makeRequester().send);
    await opened;
    socket.send("ping");
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(received).toEqual([]);
  });

  it("answers an unimplemented call with an error rather than silence", async () => {
    const { socket, received, opened } = openSocket(makeRequester().send);
    await opened;
    socket.send(JSON.stringify({ call: "networking.device", params: {}, uid: "u9" }));
    const reply = await nextMessage(received);
    expect(reply.uid).toBe("u9");
    expect(reply.error).toMatch(/networking\.device/);
  });

  it("maps printers.find to plain name strings", async () => {
    const { socket, received, opened } = openSocket(makeRequester().send);
    await opened;
    socket.send(JSON.stringify({ call: "printers.find", params: {}, uid: "u1" }));
    expect((await nextMessage(received)).result).toEqual(["TM-T20", "Kitchen"]);
  });

  it("answers printers.getDefault with a name", async () => {
    const { socket, received, opened } = openSocket(makeRequester().send);
    await opened;
    socket.send(JSON.stringify({ call: "printers.getDefault", params: null, uid: "u1" }));
    expect((await nextMessage(received)).result).toBe("TM-T20");
  });

  it("prints, resolving the name to an escpost id and sending base64", async () => {
    const requester = makeRequester();
    const { socket, received, opened } = openSocket(requester.send);
    await opened;
    socket.send(
      JSON.stringify({
        call: "print",
        params: { printer: { name: "TM-T20" }, options: { copies: 1 }, data: ["\x1b@hello"] },
        signature: "",
        signAlgorithm: "SHA1",
        uid: "u1",
      }),
    );
    const reply = await nextMessage(received);
    expect(reply.uid).toBe("u1");
    expect(reply.error).toBeUndefined();
    expect(requester.calls.find((call) => call.op === "print")!.payload).toEqual({
      printer: "tm-t20",
      data: "G0BoZWxsbw==",
    });
  });

  it("rejects a pixel/html job with an error naming @escpost/browser", async () => {
    const requester = makeRequester();
    const { socket, received, opened } = openSocket(requester.send);
    await opened;
    socket.send(
      JSON.stringify({
        call: "print",
        params: { printer: { name: "TM-T20" }, options: {}, data: [{ type: "pixel", format: "html", data: "https://x.test/r" }] },
        uid: "u1",
      }),
    );
    const reply = await nextMessage(received);
    expect(reply.error).toMatch(/@escpost\/browser/);
    expect(requester.calls.some((call) => call.op === "print")).toBe(false);
  });

  it("accepts a signed call with an empty signature, which is what QZ actually sends", async () => {
    const { socket, received, opened } = openSocket(makeRequester().send);
    await opened;
    socket.send(JSON.stringify({ call: "printers.find", params: {}, signature: "", signAlgorithm: "SHA1", uid: "u1" }));
    expect((await nextMessage(received)).error).toBeUndefined();
  });
});

describe("close()", () => {
  it("invokes onclose with `this` bound to the socket, or disconnect() hangs forever", async () => {
    const { socket, opened } = openSocket(makeRequester().send);
    await opened;

    // Exactly what qz-tray.js:1271-1286 does: set .promise on the socket, then close.
    const resolved = vi.fn();
    socket.promise = { resolve: resolved, reject: vi.fn() };
    socket.onclose = function (this: any, event: { code: number }) {
      // qz-tray.js:194-211 reads `this.promise` here.
      this.promise.resolve(event.code);
    };

    socket.close();
    for (let attempt = 0; attempt < 50 && resolved.mock.calls.length === 0; attempt++) {
      await new Promise((resolve) => setTimeout(resolve, 1));
    }
    expect(resolved).toHaveBeenCalledWith(1000);
    expect(socket.readyState).toBe(3);
  });

  it("passes a close code and reason through, including QZ's own 4003", async () => {
    const { socket, opened } = openSocket(makeRequester().send);
    await opened;
    const seen: any[] = [];
    socket.onclose = (event: unknown) => seen.push(event);
    socket.close(4003, "Connected to incompatible QZ Tray version");
    for (let attempt = 0; attempt < 50 && seen.length === 0; attempt++) {
      await new Promise((resolve) => setTimeout(resolve, 1));
    }
    expect(seen[0].code).toBe(4003);
    expect(seen[0].reason).toBe("Connected to incompatible QZ Tray version");
  });
});
