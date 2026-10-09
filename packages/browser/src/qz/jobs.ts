import { EscpostError } from "../errors";

/**
 * The version both shims report for `getVersion`.
 *
 * Not cosmetic. qz-tray.js parses the answer into a semver array and rewrites
 * outgoing jobs for servers below certain versions. 2.2.4 is the threshold of
 * the last such rewrite, still true in the current 2.2.6 client, so reporting
 * it turns every one of them off and we receive the data verbatim.
 */
export const QZ_VERSION = "2.2.4";

const USE_BROWSER_PACKAGE =
  "escpost prints raw ESC/POS only. For HTML or image receipts use @escpost/browser " +
  "(escpost.print({ printer, html })), which renders server-side.";

export interface QzPrintElement {
  type?: unknown;
  format?: unknown;
  flavor?: unknown;
  data?: unknown;
  options?: Record<string, unknown>;
}

export interface QzPrintParams {
  printer?: unknown;
  options?: Record<string, unknown>;
  data?: unknown;
}

/**
 * refuse anything that needs rendering, and refuse it by pattern alone.
 *
 * This must run before we look at `data`. By the time the message reaches us
 * `_qz.tools.relative` (qz-tray.js:695-729) has already rewritten a pixel job's
 * `data` into an absolute URL, so the payload is a link we must not follow.
 */
function assertRaw(element: QzPrintElement): void {
  const type = String(element.type ?? "raw").toLowerCase();
  const format = String(element.format ?? "command").toLowerCase();

  if (type !== "raw") {
    throw new EscpostError("UNSUPPORTED_FORMAT", `escpost cannot print a "${type}" job. ${USE_BROWSER_PACKAGE}`);
  }
  if (format !== "command") {
    throw new EscpostError(
      "UNSUPPORTED_FORMAT",
      `escpost cannot print the "${format}" format, which has to be rendered first. ${USE_BROWSER_PACKAGE}`,
    );
  }
}

/**
 * A `plain` raw string is a byte string: QZ hands code page bytes straight through
 * as characters 0x00-0xFF. Anything above that was never a byte, and picking an
 * encoding for the caller is how a receipt misprints silently.
 */
function latin1ToBytes(text: string): Uint8Array {
  const bytes = new Uint8Array(text.length);
  for (let index = 0; index < text.length; index++) {
    const code = text.charCodeAt(index);
    if (code > 0xff) {
      throw new EscpostError(
        "UNSUPPORTED_FORMAT",
        `The character "${text[index]}" is outside the single-byte range escpost sends as raw data. ` +
          "Encode the receipt in your printer's code page and send it with the base64 flavor.",
      );
    }
    bytes[index] = code;
  }
  return bytes;
}

function base64ToBytes(text: string): Uint8Array {
  let binary: string;
  try {
    binary = atob(text);
  } catch {
    throw new EscpostError("UNSUPPORTED_FORMAT", "The base64 flavor was given data that is not valid base64.");
  }
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index++) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

function hexToBytes(text: string): Uint8Array {
  const digits = text.replace(/0x/gi, "").replace(/[\s:,_-]/g, "");
  if (!/^[0-9a-f]*$/i.test(digits)) {
    throw new EscpostError("UNSUPPORTED_FORMAT", "The hex flavor was given data that is not hexadecimal.");
  }
  if (digits.length % 2 !== 0) {
    throw new EscpostError("UNSUPPORTED_FORMAT", "The hex flavor needs whole bytes: an even number of digits.");
  }
  const bytes = new Uint8Array(digits.length / 2);
  for (let index = 0; index < bytes.length; index++) {
    bytes[index] = Number.parseInt(digits.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

function elementToBytes(element: QzPrintElement): Uint8Array {
  assertRaw(element);

  const flavor = String(element.flavor ?? "plain").toLowerCase();
  if (flavor === "file" || flavor === "xml") {
    throw new EscpostError(
      "UNSUPPORTED_FORMAT",
      `The "${flavor}" flavor asks escpost to fetch the receipt from somewhere else, which it will not do. ` +
        "Read the content in the page and send it with the base64 flavor instead.",
    );
  }

  const data = element.data;
  if (typeof data !== "string") {
    throw new EscpostError("UNSUPPORTED_FORMAT", "A raw print element must carry its data as a string.");
  }

  switch (flavor) {
    case "plain":
      return latin1ToBytes(data);
    case "base64":
      return base64ToBytes(data);
    case "hex":
      return hexToBytes(data);
    default:
      throw new EscpostError("UNSUPPORTED_FORMAT", `escpost does not know the raw flavor "${flavor}".`);
  }
}

function concat(chunks: Uint8Array[]): Uint8Array {
  const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const merged = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    merged.set(chunk, offset);
    offset += chunk.length;
  }
  return merged;
}

/** Turn one QZ `print` params object into the exact bytes to hand the daemon. */
export function jobToBytes(params: QzPrintParams): Uint8Array {
  const elements = Array.isArray(params.data) ? (params.data as unknown[]) : [];

  const chunks = elements.map((element) =>
    // qz-tray.js does NOT expand a bare string client-side: both `_qz.tools.relative`
    // and `_qz.compatible.data` test `constructor === Object` and skip everything else.
    // The server has always done that expansion, so now we do.
    typeof element === "string"
      ? elementToBytes({ type: "raw", format: "command", flavor: "plain", data: element })
      : elementToBytes((element ?? {}) as QzPrintElement),
  );

  // `options` arrives with every default key set, so a present value proves nothing
  // about intent. `copies` is the only one that changes the bytes: a raw printer has
  // no driver to collate for it, so N copies means the payload N times.
  const requested = Number(params.options?.["copies"] ?? 1);
  const copies = Number.isFinite(requested) ? Math.max(1, Math.floor(requested)) : 1;

  const single = concat(chunks);
  return copies === 1 ? single : concat(new Array<Uint8Array>(copies).fill(single));
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

/**
 * `qz.configs.create("TM-T20")` auto-wraps the string into `{name: "TM-T20"}`
 * (qz-tray.js:1093-1095), but the same field also carries `{file}` and `{host, port}`.
 */
export function printerNameFrom(printer: unknown): string {
  if (typeof printer === "string") return printer;

  if (printer !== null && typeof printer === "object") {
    const target = printer as { name?: unknown; file?: unknown; host?: unknown };
    if (typeof target.name === "string") return target.name;
    if (target.file !== undefined) {
      throw new EscpostError(
        "UNSUPPORTED_FORMAT",
        "escpost does not print to a file. Give qz.configs.create() a printer name.",
      );
    }
    if (target.host !== undefined) {
      throw new EscpostError(
        "UNSUPPORTED_FORMAT",
        "escpost does not print to a raw host:port socket. Give qz.configs.create() a printer name.",
      );
    }
  }

  throw new EscpostError("PRINTER_NOT_FOUND", "The print job named no printer.");
}
