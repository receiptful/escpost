import { request } from "../../packages/browser/src/transport";
import { resolvePrinterName } from "./aliases";
import type { DaemonPrinter } from "./daemon";
import { bytesToBase64, jobToBytes, printerNameFrom, QZ_VERSION, type QzPrintParams } from "../../packages/browser/src/qz/jobs";

/** qz-tray.js:68-71 — four secure ports, then four insecure. All user-configurable, all ours. */
export const QZ_PORTS = [8181, 8282, 8383, 8484, 8182, 8283, 8384, 8485];

const PRINT_TIMEOUT_MS = 20_000;

type Requester = <T>(op: string, payload: unknown, options?: { timeoutMs?: number }) => Promise<T>;

/**
 * QZ Tray runs on the operator's own machine, so a QZ socket is always to
 * loopback. Matching on the port alone would take over a page's connection to
 * its own server on wss://relay.example.com:8181 and answer it with QZ frames —
 * a port in ordinary use for WebSocket services that has nothing to do with us.
 */
const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]", "::1"]);

function isQzUrl(url: unknown): boolean {
  try {
    const parsed = new URL(String(url));
    if (parsed.protocol !== "ws:" && parsed.protocol !== "wss:") return false;
    if (!LOCAL_HOSTS.has(parsed.hostname.toLowerCase())) return false;
    return QZ_PORTS.includes(Number(parsed.port));
  } catch {
    // Not a URL we can parse is not a URL QZ produced.
    return false;
  }
}

async function invoke(send: Requester, call: string, params: QzPrintParams & { query?: unknown }): Promise<unknown> {
  switch (call) {
    case "getVersion":
      return QZ_VERSION;

    case "printers.find": {
      const printers = await send<DaemonPrinter[]>("printers.list", undefined);
      const query = params?.query;
      // QZ hands back plain name strings, never objects.
      if (typeof query !== "string") return printers.map((printer) => printer.name);

      const matchedId = resolvePrinterName(query, printers, {});
      const matched = matchedId === null ? undefined : printers.find((printer) => printer.id === matchedId);
      if (matched === undefined) {
        const known = printers.map((printer) => printer.name).join(", ");
        throw new Error(`No printer matches "${query}". escpost knows: ${known || "(none configured)"}.`);
      }
      return matched.name;
    }

    case "printers.getDefault": {
      const preferred = await send<DaemonPrinter | null>("printers.default", undefined);
      return preferred?.name ?? null;
    }

    case "print": {
      const requested = printerNameFrom(params?.printer);
      // Translate first: a job we cannot print must never reach the daemon.
      const payload = bytesToBase64(jobToBytes(params ?? {}));

      const printers = await send<DaemonPrinter[]>("printers.list", undefined);
      // The alias map and unmatched-name record live in extension storage,
      // out of reach here, so an unresolved name goes to the worker verbatim.
      const resolved = resolvePrinterName(requested, printers, {});

      await send<{ jobId: string }>(
        "print",
        { printer: resolved ?? requested, data: payload },
        { timeoutMs: PRINT_TIMEOUT_MS },
      );
      return null;
    }

    default:
      // `qz.websocket.getNetworkInfo()` lands here as `networking.device`, along with
      // serial, usb, hid, file and socket. An error reply is mandatory: a call left
      // unanswered leaves the page's promise pending for the life of the tab.
      throw new Error(`escpost does not implement the QZ Tray call "${call}".`);
  }
}

export function createPatchedWebSocket(Native: typeof WebSocket | undefined, send: Requester) {
  return class EscpostWebSocket {
    // qz-tray.js:1211 rejects the whole connection if CLOSED is missing or is 2.
    static readonly CONNECTING = 0;
    static readonly OPEN = 1;
    static readonly CLOSING = 2;
    static readonly CLOSED = 3;

    // Initialised here, not only in the constructor: the passthrough branch returns a
    // native socket before the body assigns them, which strictPropertyInitialization
    // cannot see through. The constructor overwrites both for an intercepted URL.
    url = "";
    readyState: number = EscpostWebSocket.CONNECTING;
    onopen: ((this: EscpostWebSocket, event: unknown) => void) | null = null;
    onclose: ((this: EscpostWebSocket, event: unknown) => void) | null = null;
    onerror: ((this: EscpostWebSocket, event: unknown) => void) | null = null;
    onmessage: ((this: EscpostWebSocket, event: { data: string }) => void) | null = null;

    // qz-tray.js writes .established, .interval, .version, .semver and .promise
    // directly onto the socket, so this stays an ordinary mutable object.
    [key: string]: unknown;

    constructor(url: string | URL, protocols?: string | string[]) {
      if (!isQzUrl(url)) {
        // everything that is not QZ gets a real socket, unchanged.
        if (!Native) throw new Error("WebSocket is not available in this context.");
        return new Native(url as string, protocols) as unknown as EscpostWebSocket;
      }

      this.url = String(url);
      this.readyState = EscpostWebSocket.CONNECTING;

      setTimeout(() => {
        if (this.readyState !== EscpostWebSocket.CONNECTING) return;
        this.readyState = EscpostWebSocket.OPEN;
        this.onopen?.call(this, { type: "open", target: this });
      }, 0);
    }

    send(raw: unknown): void {
      // The keep-alive is the bare string "ping" every `keepAlive` seconds, default 60
      // (qz-tray.js:153-164). It is not JSON and wants no answer: "pong" would throw
      // inside JSON.parse, and a JSON reply with no uid would trip the 4003 close.
      if (raw === "ping") return;

      let message: { call?: unknown; uid?: unknown; params?: unknown };
      try {
        message = JSON.parse(String(raw));
      } catch {
        return;
      }
      void this.#dispatch(message);
    }

    async #dispatch(message: { call?: unknown; uid?: unknown; params?: unknown }): Promise<void> {
      const uid = message.uid;
      // Nothing is pending without a uid, and a reply without one closes the socket.
      if (typeof uid !== "string") return;

      try {
        if (message.call === undefined) {
          // the handshake message carries a certificate and no call (qz-tray.js:381).
          // The reply to this uid is what resolves connect(); an error here rejects it.
          // The certificate is null in the normal case, because the default cert handler
          // rejects and rejectOnCertFailure is false. We do not look at it.
          this.#reply({ uid, result: "Connected to escpost" });
          return;
        }
        const result = await invoke(send, String(message.call), (message.params ?? {}) as QzPrintParams);
        this.#reply({ uid, result });
      } catch (error) {
        // qz-tray.js:335-341 rejects with `new Error(returned.error)`, so a string is right.
        this.#reply({ uid, error: error instanceof Error ? error.message : String(error) });
      }
    }

    #reply(body: { uid: string; result?: unknown; error?: string }): void {
      if (this.readyState !== EscpostWebSocket.OPEN) return;
      this.onmessage?.call(this, { type: "message", data: JSON.stringify(body), target: this });
    }

    close(code = 1000, reason = ""): void {
      if (this.readyState === EscpostWebSocket.CLOSED || this.readyState === EscpostWebSocket.CLOSING) return;
      this.readyState = EscpostWebSocket.CLOSING;

      setTimeout(() => {
        this.readyState = EscpostWebSocket.CLOSED;
        // `.call(this, ...)` is the whole point: qz-tray.js:194-211 reads `this.promise`
        // inside onclose to settle disconnect(). Lose the binding and disconnect() hangs.
        this.onclose?.call(this, { type: "close", code, reason, wasClean: code === 1000, target: this });
      }, 0);
    }

    addEventListener(): void {}
    removeEventListener(): void {}
  };
}

const MARKER = "__escpostWebSocketPatched";

export function installWebSocketPatch(target: Record<string, unknown> = globalThis as unknown as Record<string, unknown>): boolean {
  if (target[MARKER] === true) return false;
  const native = target["WebSocket"] as typeof WebSocket | undefined;
  target["WebSocket"] = createPatchedWebSocket(native, request as Requester);
  target[MARKER] = true;
  return true;
}

// qz-tray.js:681 captures WebSocket at evaluation time, so this file is a
// MAIN-world content script at document_start and installs on import. Later is inert.
installWebSocketPatch();
