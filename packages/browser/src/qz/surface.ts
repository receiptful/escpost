import { EscpostError } from "../errors";
import { request } from "../transport";
import type { Printer } from "../types";
import { bytesToBase64, jobToBytes, printerNameFrom, QZ_VERSION } from "./jobs";

/**
 * Match a requested name against what escpost reports, on id or name alone.
 *
 * The extension resolves user-defined aliases too, but only in its worker: this
 * code runs in the page, which cannot read extension storage. An unresolved
 * name is forwarded untouched and the worker applies aliases afterwards, so
 * matching here is an optimisation rather than the last word.
 */
function matchPrinter(requested: string, printers: Printer[]): Printer | undefined {
  const needle = requested.trim().toLowerCase();
  return printers.find(
    (printer) => printer.id.toLowerCase() === needle || printer.name.toLowerCase() === needle,
  );
}

/** A raw job may wait on the daemon opening a USB device; match the package's budget. */
const PRINT_TIMEOUT_MS = 20_000;

type QzOptions = Record<string, unknown>;

/** The same defaults qz-tray.js:450-475 copies into every new config. */
const DEFAULT_OPTIONS: QzOptions = {
  bounds: null,
  colorType: "color",
  copies: 1,
  density: 0,
  duplex: false,
  fallbackDensity: null,
  interpolation: "bicubic",
  jobName: null,
  legacy: false,
  margins: 0,
  orientation: null,
  paperThickness: null,
  printerTray: null,
  rasterize: false,
  rotation: 0,
  scaleContent: true,
  size: null,
  units: "in",
  forceRaw: false,
  encoding: null,
  spool: null,
};

export class ShimConfig {
  printer: unknown;
  private options: QzOptions;

  constructor(printer: unknown, options?: QzOptions) {
    this.printer = typeof printer === "string" ? { name: printer } : printer;
    this.options = { ...DEFAULT_OPTIONS, ...(options ?? {}) };
  }

  setPrinter(printer: unknown): void {
    this.printer = typeof printer === "string" ? { name: printer } : printer;
  }

  getPrinter(): unknown {
    return this.printer;
  }

  reconfigure(options: QzOptions): void {
    Object.assign(this.options, options);
  }

  getOptions(): QzOptions {
    return this.options;
  }
}

/**
 * Resolve whatever the page asked for and send the job.
 *
 * Resolution here is on names and ids alone, and an unresolved name is
 * forwarded untouched for the worker to alias.
 */
async function sendJob(config: ShimConfig, elements: unknown[]): Promise<void> {
  const requested = printerNameFrom(config.getPrinter());

  // Translate before any round trip, so an HTML job fails without touching the daemon.
  const payload = bytesToBase64(
    jobToBytes({ printer: config.getPrinter(), options: config.getOptions(), data: elements }),
  );

  const printers = await request<Printer[]>("printers.list", undefined);
  const resolved = matchPrinter(requested, printers);

  await request<{ jobId: string }>(
    "print",
    { printer: resolved?.id ?? requested, data: payload },
    { timeoutMs: PRINT_TIMEOUT_MS },
  );
}

export function createQzShim() {
  let connected = false;
  let closedCallbacks: Array<(event: unknown) => void> = [];
  let errorCallbacks: Array<(event: unknown) => void> = [];

  function assertActive(): void {
    // The same words qz-tray.js:797 uses, so an integration matching on the message still matches.
    if (!connected) throw new Error("A connection to QZ has not been established yet");
  }

  const qz = {
    version: QZ_VERSION,

    websocket: {
      connect(): Promise<void> {
        if (connected) return Promise.reject(new Error("An open connection with QZ Tray already exists"));
        connected = true;
        return Promise.resolve();
      },

      isActive(): boolean {
        return connected;
      },

      disconnect(): Promise<void> {
        if (!connected) return Promise.reject(new Error("No open connection with QZ Tray"));
        connected = false;
        for (const callback of closedCallbacks) callback({ type: "close", code: 1000, reason: "" });
        return Promise.resolve();
      },

      // There is no socket. Integrations log this; none of them can act on it.
      getConnectionInfo(): { socket: string; host: string; port: number } {
        assertActive();
        return { socket: "escpost", host: "127.0.0.1", port: 0 };
      },

      setClosedCallbacks(calls: ((event: unknown) => void) | Array<(event: unknown) => void>): void {
        closedCallbacks = Array.isArray(calls) ? calls : [calls];
      },

      setErrorCallbacks(calls: ((event: unknown) => void) | Array<(event: unknown) => void>): void {
        errorCallbacks = Array.isArray(calls) ? calls : [calls];
      },
    },

    api: {
      getVersion(): Promise<string> {
        return Promise.resolve(QZ_VERSION);
      },
      // present and inert. A page calling these before printing must not crash.
      setPromiseType(): void {},
      setSha256Type(): void {},
      setWebSocketType(): void {},
      showDebug(): void {},
    },

    /**
     * escpost has no signing to do. Every setter exists, succeeds, and prompts
     * nothing — including the certificate handler, whose result is simply discarded.
     */
    security: {
      setCertificatePromise(): void {},
      setSignaturePromise(): void {},
      setSignatureAlgorithm(): void {},
    },

    printers: {
      async find(query?: string | null): Promise<string[] | string> {
        assertActive();
        const printers = await request<Printer[]>("printers.list", undefined);
        if (query === undefined || query === null) return printers.map((printer) => printer.name);

        const matched = matchPrinter(query, printers);
        if (matched === undefined) {
          // the migration failure is a name mismatch, so say which names exist.
          const known = printers.map((printer) => printer.name).join(", ");
          throw new EscpostError(
            "PRINTER_NOT_FOUND",
            `No printer matches "${query}". escpost knows: ${known || "(none configured)"}.`,
          );
        }
        return matched.name;
      },

      async getDefault(): Promise<string | null> {
        assertActive();
        const preferred = await request<Printer | null>("printers.default", undefined);
        return preferred?.name ?? null;
      },
    },

    configs: {
      create(printer: unknown, options?: QzOptions): ShimConfig {
        return new ShimConfig(printer, options);
      },
      setDefaults(options: QzOptions): void {
        Object.assign(DEFAULT_OPTIONS, options);
      },
    },

    // Resolves null, not undefined: qz-tray.js returns the last call's result
    // (qz-tray.js:1688-1694), and a `print` result is null.
    async print(configs: ShimConfig | ShimConfig[], data: unknown[] | unknown[][]): Promise<null> {
      assertActive();

      // qz.print accepts one config or many, and one data array or an array of them.
      const configList = Array.isArray(configs) ? configs : [configs];
      const dataList = (Array.isArray(data[0]) ? data : [data]) as unknown[][];

      for (let index = 0; index < Math.max(configList.length, dataList.length); index++) {
        const config = configList[Math.min(index, configList.length - 1)]!;
        const elements = dataList[Math.min(index, dataList.length - 1)]!;
        await sendJob(config, elements);
      }
      return null;
    },
  };

  void errorCallbacks; // Registered for API parity; escpost has no socket to error on.
  return qz;
}

/**
 * install only into a page that has no `qz` yet, and install a plain writable
 * property, so a page that later loads the real qz-tray.js overwrites us cleanly and
 * is served by the WebSocket patch instead.
 */
export function installQzShim(target: Window & { qz?: unknown } = window): boolean {
  if (target.qz !== undefined) return false;
  target.qz = createQzShim();
  return true;
}
