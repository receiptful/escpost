import { EscpostError } from "../errors";
import { escpost } from "../index";
import type { Printer } from "../types";
import { jobToBytes, printerNameFrom, QZ_VERSION } from "./jobs";
import { renderElements, type PrinterGeometry } from "./render";

/** The part of the ESCPost client the shim uses. Tests pass a fake one. */
export type EscpostClient = Pick<typeof escpost, "isAvailable" | "printers" | "profiles" | "print">;

/**
 * Match a requested name against the ESCPost printer names the way QZ Tray's
 * `PrintServiceMatcher.matchPrinter` does: ignoring case, the exact name first,
 * then a name that starts with the query, then one that contains it, so
 * QZ's `find("zebra")` finds "Zebra LP2844". Like QZ, the last of several
 * equally good matches wins. Surrounding spaces are ignored too, because QZ
 * pages often type the name by hand.
 */
function matchPrinter(requested: string, printers: Printer[]): Printer | undefined {
  const needle = requested.trim().toLowerCase();
  let begins: Printer | undefined;
  let partial: Printer | undefined;
  for (const printer of printers) {
    const name = printer.name.toLowerCase();
    if (name === needle) return printer;
    if (name.startsWith(needle)) begins = printer;
    else if (name.includes(needle)) partial = printer;
  }
  return begins ?? partial;
}

/**
 * The options of QZ's `qz.websocket.connect()` (qz.io/api/qz.websocket). There
 * is no socket to open, so the shim accepts them and ignores them.
 */
export type QzConnectOptions = {
  host?: string | string[];
  port?: { secure?: number[]; insecure?: number[] };
  usingSecure?: boolean;
  keepAlive?: number;
  retries?: number;
  delay?: number;
};

/** A QZ callback setter the shim accepts and ignores, whatever QZ passes it. */
type IgnoredSetter = (...calls: unknown[]) => void;
const ignoredSetter: IgnoredSetter = () => {};

/** QZ functions ESCPost does not offer: each one rejects, naming itself. */
function unsupportedMethods<const Name extends string>(
  namespace: string,
  names: readonly Name[],
): Record<Name, (...args: unknown[]) => Promise<never>> {
  // The const type parameter keeps each name in the type, so `qz.usb.listDevices` type-checks.
  return Object.fromEntries(
    names.map((name) => [
      name,
      () =>
        Promise.reject(
          new EscpostError("UNSUPPORTED_FEATURE", `qz.${namespace}.${name}() is a QZ Tray feature that ESCPost does not offer.`),
        ),
    ]),
  ) as Record<Name, (...args: unknown[]) => Promise<never>>;
}

function unknownPrinter(requested: string, printers: Printer[]): EscpostError {
  // The usual migration failure is a name mismatch, so say which names exist.
  const known = printers.map((printer) => printer.name).join(", ");
  return new EscpostError(
    "PRINTER_NOT_FOUND",
    `No printer matches "${requested}". ESCPost knows: ${known || "(none configured)"}.`,
  );
}

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

type PrintFunction = (configs: ShimConfig, data: unknown[] | unknown[][], ...rest: unknown[]) => Promise<null>;

export class ShimConfig {
  printer: unknown;
  private options: QzOptions;
  private readonly printWith: PrintFunction;

  constructor(printer: unknown, options: QzOptions | undefined, print: PrintFunction) {
    this.printer = typeof printer === "string" ? { name: printer } : printer;
    this.options = { ...DEFAULT_OPTIONS, ...(options ?? {}) };
    this.printWith = print;
  }

  /**
   * QZ's shortcut for `qz.print(config, data, ...)` (qz-tray.js `Config.prototype.print`).
   * QZ drops the promise; returning it lets the page wait for the job and catch its error.
   */
  print(data: unknown[] | unknown[][], ...rest: unknown[]): Promise<null> {
    return this.printWith(this, data, ...rest);
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

/** Resolve the printer the page asked for, then send the job's bytes to ESCPost. */
/**
 * The printable width and resolution of `printer`, from its ESCPost profile, or
 * undefined when it has none (or the catalog no longer holds it), in which case
 * render.ts uses ESCPost's safe default. A printer without a profile costs no
 * round trip.
 */
export async function printerGeometry(client: EscpostClient, printer: Printer): Promise<PrinterGeometry | undefined> {
  if (printer.profile === null) return undefined;
  const { profiles } = await client.profiles.list();
  return profiles.find((profile) => profile.id === printer.profile);
}

/** Resolve the printer the page asked for, then draw the job for it and send its bytes. */
async function sendJob(client: EscpostClient, config: ShimConfig, elements: unknown[]): Promise<void> {
  const requested = printerNameFrom(config.getPrinter());
  const { printers } = await client.printers.list();
  const resolved = matchPrinter(requested, printers);
  if (resolved === undefined) throw unknownPrinter(requested, printers);

  // Images, HTML, PDF and files become raw ESC/POS first, drawn for this
  // printer's width and resolution (see render.ts), then everything becomes bytes.
  let geometry: Promise<PrinterGeometry | undefined> | undefined;
  const rendered = await renderElements(elements, config.getOptions(), () => (geometry ??= printerGeometry(client, resolved)));
  const data = jobToBytes({ printer: config.getPrinter(), options: config.getOptions(), data: rendered });

  await client.print({ printer: resolved.name, data });
}

export function createQzShim(client: EscpostClient = escpost) {
  let connected = false;
  let closedCallbacks: Array<(event: unknown) => void> = [];
  let errorCallbacks: Array<(event: unknown) => void> = [];

  function assertActive(): void {
    // The same words qz-tray.js:797 uses, so an integration matching on the message still matches.
    if (!connected) throw new Error("A connection to QZ has not been established yet");
  }

  // Resolves null, not undefined: qz-tray.js returns the last call's result
  // (qz-tray.js:1688-1694), and a `print` result is null. QZ's optional
  // resumeOnError and signature arguments are accepted and ignored.
  async function print(
    configs: ShimConfig | ShimConfig[],
    data: unknown[] | unknown[][],
    ..._ignored: unknown[]
  ): Promise<null> {
    assertActive();

    // qz.print accepts one config or many, and one data array or an array of them.
    const configList = Array.isArray(configs) ? configs : [configs];
    const dataList = (Array.isArray(data[0]) ? data : [data]) as unknown[][];

    for (let index = 0; index < Math.max(configList.length, dataList.length); index++) {
      const config = configList[Math.min(index, configList.length - 1)]!;
      const elements = dataList[Math.min(index, dataList.length - 1)]!;
      await sendJob(client, config, elements);
    }
    return null;
  }

  const qz = {
    version: QZ_VERSION,

    websocket: {
      async connect(_options?: QzConnectOptions): Promise<void> {
        if (connected) throw new Error("An open connection with QZ Tray already exists");
        // QZ fails here when the QZ Tray app is not running, so fail the same way
        // when the extension, escpost serve or the site's permission is missing.
        if (!(await client.isAvailable())) {
          throw new EscpostError(
            "DAEMON_UNAVAILABLE",
            "ESCPost is not available. Install the ESCPost extension, run escpost serve, " +
              "and allow this site in the extension popup.",
          );
        }
        connected = true;
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
      setPromiseType(_promiser?: unknown): void {},
      setSha256Type(_hasher?: unknown): void {},
      setWebSocketType(_webSocket?: unknown): void {},
      showDebug(_show?: boolean): void {},
    },

    /**
     * escpost has no signing to do. Every setter exists, succeeds, and prompts
     * nothing — including the certificate handler, whose result is simply discarded.
     */
    security: {
      setCertificatePromise(_handler?: unknown, _options?: unknown): void {},
      setSignaturePromise(_factory?: unknown): void {},
      setSignatureAlgorithm(_algorithm?: string): void {},
    },

    printers: {
      // QZ's optional signature and timestamp arguments are accepted and ignored.
      async find(query?: string | null, ..._signing: unknown[]): Promise<string[] | string> {
        assertActive();
        const { printers } = await client.printers.list();
        if (query === undefined || query === null) return printers.map((printer) => printer.name);

        const matched = matchPrinter(query, printers);
        if (matched === undefined) throw unknownPrinter(query, printers);
        return matched.name;
      },

      // ESCPost has no default printer, so the first connected one plays that role.
      async getDefault(..._signing: unknown[]): Promise<string | null> {
        assertActive();
        const { printers } = await client.printers.list();
        return printers.find((printer) => printer.availability === "connected")?.name ?? null;
      },

      // Printer details, status and job queues come from the OS print system,
      // which ESCPost does not use.
      ...unsupportedMethods("printers", ["details", "startListening", "stopListening", "getStatus", "clearQueue"]),
      setPrinterCallbacks: ignoredSetter,
    },

    // QZ Tray also talks to serial ports, sockets, USB and HID devices, files and
    // the network. ESCPost does not. Each namespace still exists, so a page that
    // registers its callbacks on load keeps working; calling one of its
    // functions rejects with a clear message instead of crashing the page.
    serial: {
      ...unsupportedMethods("serial", ["findPorts", "openPort", "sendData", "closePort"]),
      setSerialCallbacks: ignoredSetter,
    },
    socket: {
      ...unsupportedMethods("socket", ["open", "close", "sendData"]),
      setSocketCallbacks: ignoredSetter,
    },
    usb: {
      ...unsupportedMethods("usb", [
        "listDevices", "listInterfaces", "listEndpoints", "claimDevice", "isClaimed",
        "sendData", "readData", "openStream", "closeStream", "releaseDevice",
      ]),
      setUsbCallbacks: ignoredSetter,
    },
    hid: {
      ...unsupportedMethods("hid", [
        "listDevices", "startListening", "stopListening", "claimDevice", "isClaimed", "sendData",
        "readData", "sendFeatureReport", "getFeatureReport", "openStream", "closeStream", "releaseDevice",
      ]),
      setHidCallbacks: ignoredSetter,
    },
    file: {
      ...unsupportedMethods("file", ["list", "read", "write", "remove", "startListening", "stopListening"]),
      setFileCallbacks: ignoredSetter,
    },
    networking: unsupportedMethods("networking", ["device", "hostname", "devices"]),

    configs: {
      create(printer: unknown, options?: QzOptions): ShimConfig {
        return new ShimConfig(printer, options, print);
      },
      setDefaults(options: QzOptions): void {
        Object.assign(DEFAULT_OPTIONS, options);
      },
    },

    print,
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
