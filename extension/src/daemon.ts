import { EscpostError, type ErrorCode } from "../../packages/browser/src/errors";

const NOT_RUNNING =
  "escpost is not running on this machine. Start it, or install it from escpost.dev.";

interface Options {
  attempts?: number;
  backoffMs?: number;
}

/**
 * A printer as the extension uses it.
 *
 * escpost serves a richer shape than this: the workbench needs USB topology to
 * draw a device picker, and reports availability rather than status. That is
 * escpost's contract with its own app and not something to reshape, so this
 * client narrows it here and the rest of the extension sees one stable form.
 */
export interface DaemonPrinter {
  /** The printers.toml key, which is also what a job is addressed to. */
  id: string;
  name: string;
  transport: "usb" | "network";
  profile: string | null;
  status: "ready" | "unavailable";
  device?: DaemonDeviceFacts;
}

/** The fields a device fingerprint is built from, lifted out of `connection`. */
export interface DaemonDeviceFacts {
  usbVendorId?: number;
  usbProductId?: number;
  usbSerial?: string;
  host?: string;
  port?: number;
}

/** What escpost actually sends. Only the fields this client reads are typed. */
interface ListedPrinter {
  name: string;
  transport: "usb" | "network";
  availability: "connected" | "unavailable";
  profile: string | null;
  connection:
    | { type: "usb"; vendor_id: number; product_id: number; serial_number: string | null }
    | { type: "network"; host: string; port: number };
}

function toDaemonPrinter(listed: ListedPrinter): DaemonPrinter {
  return {
    id: listed.name,
    name: listed.name,
    transport: listed.transport,
    profile: listed.profile,
    status: listed.availability === "connected" ? "ready" : "unavailable",
    device:
      listed.connection.type === "usb"
        ? {
            usbVendorId: listed.connection.vendor_id,
            usbProductId: listed.connection.product_id,
            ...(listed.connection.serial_number === null
              ? {}
              : { usbSerial: listed.connection.serial_number }),
          }
        : { host: listed.connection.host, port: listed.connection.port },
  };
}

/**
 * Stateless by construction: every call is one fetch, so a recycled service
 * worker has nothing to restore. Retries cover a daemon restart.
 */
export class DaemonClient {
  readonly #base: string | (() => Promise<string>);
  readonly #fetch: typeof fetch;
  readonly #attempts: number;
  readonly #backoffMs: number;

  constructor(
    base: string | (() => Promise<string>),
    fetchImpl: typeof fetch = fetch,
    options: Options = {},
  ) {
    // A function, when the caller has to find escpost before it can talk to it.
    this.#base = typeof base === "string" ? base.replace(/\/$/, "") : base;
    // Bound at the boundary, not at the call site. `fetch` is a WebIDL operation:
    // invoking it as a method of anything but the global -- which `this.#fetch(...)`
    // does -- throws "Illegal invocation". That threw inside the retry loop and
    // surfaced as DAEMON_NOT_RUNNING against a daemon that was running.
    this.#fetch = fetchImpl.bind(globalThis);
    this.#attempts = options.attempts ?? 3;
    this.#backoffMs = options.backoffMs ?? 150;
  }

  /** Whether escpost is answering at all. It has no version handshake: the
   *  shape below is the shape, and a client that cannot parse it is broken
   *  rather than out of date. */
  async available(): Promise<boolean> {
    // Not through #send, which parses every body as JSON. /health answers the
    // plain string "ok", so parsing it threw and this reported a healthy
    // escpost as absent.
    try {
      const base = typeof this.#base === "string" ? this.#base : await this.#base();
      const response = await this.#fetch(base + "/health", { method: "GET" });
      return response.ok;
    } catch {
      return false;
    }
  }

  async printers(): Promise<DaemonPrinter[]> {
    const body = await this.#send<{ printers?: ListedPrinter[] }>("/api/printers/list", { method: "GET" });
    // Something answered on the port but did not send a listing. That is not a
    // printer problem, and it should not surface as a TypeError from `.map`.
    if (!Array.isArray(body?.printers)) {
      throw new EscpostError("PRINT_FAILED", "escpost answered with something other than a printer list.");
    }
    return body.printers.map(toDaemonPrinter);
  }

  /** escpost has no default-printer endpoint. Its listing is ordered though,
   *  connected before unavailable and then by name, so the first entry is the
   *  same answer every time. */
  async defaultPrinter(): Promise<DaemonPrinter | null> {
    const printers = await this.printers();
    return printers[0] ?? null;
  }

  print(printer: string, base64Data: string): Promise<{ jobId: string }> {
    return this.#send("/api/print", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ printer, data: base64Data }),
    });
  }

  async #send<T>(path: string, init: RequestInit & { headers?: Record<string, string> }): Promise<T> {
    let lastTransportError: unknown = null;

    for (let attempt = 0; attempt < this.#attempts; attempt++) {
      let response: Response;
      try {
        const base = typeof this.#base === "string" ? this.#base : await this.#base();
        response = await this.#fetch(base + path, init);
      } catch (error) {
        // The daemon is down, restarting, or the port moved. Worth another try.
        lastTransportError = error;
        if (attempt < this.#attempts - 1) await sleep(this.#backoffMs * 2 ** attempt);
        continue;
      }

      if (response.ok) return (await response.json()) as T;

      // A 4xx is a verdict, not a hiccup — retrying only wastes the caller's time.
      if (response.status >= 400 && response.status < 500) throw await toError(response);

      lastTransportError = await toError(response);
      if (attempt < this.#attempts - 1) await sleep(this.#backoffMs * 2 ** attempt);
    }

    if (lastTransportError instanceof EscpostError) throw lastTransportError;
    throw new EscpostError("DAEMON_NOT_RUNNING", NOT_RUNNING);
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function toError(response: Response): Promise<EscpostError> {
  let code: ErrorCode = "PRINT_FAILED";
  let message = `The daemon returned ${response.status}.`;
  try {
    const body = (await response.json()) as { error?: { code?: string; message?: string } };
    if (body.error?.message) message = body.error.message;
    if (body.error?.code) code = body.error.code as ErrorCode;
  } catch {
    // A non-JSON error body is not itself an error worth reporting over the real one.
  }
  if (response.status === 403) {
    return new EscpostError("ORIGIN_NOT_GRANTED", "The daemon rejected this extension's origin. " + message);
  }
  return new EscpostError(code, message);
}
