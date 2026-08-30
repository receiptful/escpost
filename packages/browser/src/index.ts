import { EscpostError } from "./errors";
import { request } from "./transport";
import type { Printer, PrintRequest, PrintResult, PrintTarget } from "./types";

export { EscpostError, isEscpostError } from "./errors";
export type { Printer, PrintRequest, PrintResult, PrintTarget } from "./types";

const KNOWN_TARGETS: ReadonlySet<string> = new Set<PrintTarget>(["local"]);

/** Slow enough that a shop floor of tills does not hammer one daemon, quick
 *  enough that an operator notices a printer going offline before the customer
 *  does. The floor stops a caller asking for a request every 50ms. */
const DEFAULT_POLL_MS = 5000;
const MINIMUM_POLL_MS = 250;

/** Printing is a long-tail operation; the daemon may be opening a USB device. */
const PRINT_TIMEOUT_MS = 20_000;

/** A render round trip sits in front of the print, and the printer may still
 *  be opening a USB device after it. */
const HTML_PRINT_TIMEOUT_MS = 30_000;

export const escpost = {
  /**
   * Whether a print would work right now: the extension is installed, this
   * site is granted, and the daemon answers. Resolves false rather than
   * throwing, because the point of asking is to decide whether to offer
   * printing at all. When you need to know *why*, call printers.list() and
   * read the error code.
   */
  async isAvailable(): Promise<boolean> {
    try {
      await request<Printer[]>("printers.list", undefined);
      return true;
    } catch {
      return false;
    }
  },

  printers: {
    list(): Promise<Printer[]> {
      return request<Printer[]>("printers.list", undefined);
    },
    getDefault(): Promise<Printer | null> {
      return request<Printer | null>("printers.default", undefined);
    },

    /**
     * Call `listener` with the printer list whenever it changes, and once with
     * the current list to start. Returns a function that stops the
     * subscription.
     *
     * This polls. The page and the extension speak request and reply through
     * one relay, which has nothing to push with, so an unsolicited status
     * message has no route to arrive on. Polling keeps the signature a push
     * implementation can adopt unchanged if that route is ever built.
     *
     * A failed poll reports an empty list, because a till that cannot reach
     * its printer has no printer, and recovers on its own when the daemon
     * comes back.
     */
    subscribe(listener: (printers: Printer[]) => void, options: { intervalMs?: number } = {}): () => void {
      const intervalMs = Math.max(options.intervalMs ?? DEFAULT_POLL_MS, MINIMUM_POLL_MS);
      let previous: string | null = null;
      let stopped = false;
      let inFlight = false;

      const poll = async (): Promise<void> => {
        if (stopped || inFlight) return;
        inFlight = true;
        let printers: Printer[];
        try {
          printers = await request<Printer[]>("printers.list", undefined);
        } catch {
          printers = [];
        }
        inFlight = false;
        if (stopped) return;
        const current = JSON.stringify(printers);
        if (current === previous) return;
        previous = current;
        listener(printers);
      };

      void poll();
      const timer = setInterval(() => void poll(), intervalMs);

      return () => {
        stopped = true;
        clearInterval(timer);
      };
    },
  },

  print(job: PrintRequest): Promise<PrintResult> {
    // An unknown target must never fall through to the local printer.
    if (job.target !== undefined && !KNOWN_TARGETS.has(job.target)) {
      return Promise.reject(
        new EscpostError("PRINT_FAILED", `Unknown target "${job.target}". This build supports: local.`),
      );
    }

    if ("html" in job && job.html !== undefined) {
      // The extension renders this server-side and prints the bytes it gets
      // back. This package holds no renderer and no account logic — it forwards
      // a message and nothing more.
      return request<PrintResult>(
        "print",
        { printer: job.printer, html: job.html },
        { timeoutMs: HTML_PRINT_TIMEOUT_MS },
      );
    }

    if (!("data" in job) || job.data === undefined) {
      return Promise.reject(new EscpostError("PRINT_FAILED", "print() needs either `data` (raw ESC/POS) or `html`."));
    }

    return request<PrintResult>("print", { printer: job.printer, data: toBase64(job.data) }, { timeoutMs: PRINT_TIMEOUT_MS });
  },
};

export default escpost;

/** Raster receipts run to tens of kilobytes, so build the binary string in
 *  chunks rather than one character at a time. The chunk stays well under the
 *  argument limit that spreading a whole image into fromCharCode would hit. */
const BASE64_CHUNK_BYTES = 8192;

function toBase64(data: Uint8Array | string): string {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += BASE64_CHUNK_BYTES) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + BASE64_CHUNK_BYTES));
  }
  return btoa(binary);
}
