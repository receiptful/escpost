import { EscpostError } from "../errors";

/**
 * Text to bytes, the way QZ Tray does it.
 *
 * QZ Tray turns a `plain` raw string into bytes with Java's
 * `String.getBytes(charset)`, where the charset is the config's `encoding`
 * option (a Java name such as "IBM00858", "Cp850" or "windows-1252"). With no
 * encoding it uses Java's default charset, UTF-8 on the Java that QZ Tray 2.3
 * ships. A character the charset cannot represent becomes "?", as in Java.
 *
 * Browsers can decode most single-byte charsets with TextDecoder but encode
 * only UTF-8, and they know none of the DOS code pages that receipt printers
 * use. So the DOS pages are tables here, and the others are built once from
 * TextDecoder.
 */

const UNMAPPABLE = 0x3f; // "?", Java's replacement byte

/** Characters for bytes 0x80-0xFF. U+FFFD marks a byte the code page leaves undefined. */
const DOS_CODE_PAGES: Record<string, string> = {
  cp437:
    "\u00c7\u00fc\u00e9\u00e2\u00e4\u00e0\u00e5\u00e7\u00ea\u00eb\u00e8\u00ef\u00ee\u00ec\u00c4\u00c5\u00c9\u00e6\u00c6\u00f4\u00f6\u00f2\u00fb\u00f9\u00ff\u00d6\u00dc\u00a2\u00a3\u00a5\u20a7\u0192" +
    "\u00e1\u00ed\u00f3\u00fa\u00f1\u00d1\u00aa\u00ba\u00bf\u2310\u00ac\u00bd\u00bc\u00a1\u00ab\u00bb\u2591\u2592\u2593\u2502\u2524\u2561\u2562\u2556\u2555\u2563\u2551\u2557\u255d\u255c\u255b\u2510" +
    "\u2514\u2534\u252c\u251c\u2500\u253c\u255e\u255f\u255a\u2554\u2569\u2566\u2560\u2550\u256c\u2567\u2568\u2564\u2565\u2559\u2558\u2552\u2553\u256b\u256a\u2518\u250c\u2588\u2584\u258c\u2590\u2580" +
    "\u03b1\u00df\u0393\u03c0\u03a3\u03c3\u00b5\u03c4\u03a6\u0398\u03a9\u03b4\u221e\u03c6\u03b5\u2229\u2261\u00b1\u2265\u2264\u2320\u2321\u00f7\u2248\u00b0\u2219\u00b7\u221a\u207f\u00b2\u25a0\u00a0",
  cp850:
    "\u00c7\u00fc\u00e9\u00e2\u00e4\u00e0\u00e5\u00e7\u00ea\u00eb\u00e8\u00ef\u00ee\u00ec\u00c4\u00c5\u00c9\u00e6\u00c6\u00f4\u00f6\u00f2\u00fb\u00f9\u00ff\u00d6\u00dc\u00f8\u00a3\u00d8\u00d7\u0192" +
    "\u00e1\u00ed\u00f3\u00fa\u00f1\u00d1\u00aa\u00ba\u00bf\u00ae\u00ac\u00bd\u00bc\u00a1\u00ab\u00bb\u2591\u2592\u2593\u2502\u2524\u00c1\u00c2\u00c0\u00a9\u2563\u2551\u2557\u255d\u00a2\u00a5\u2510" +
    "\u2514\u2534\u252c\u251c\u2500\u253c\u00e3\u00c3\u255a\u2554\u2569\u2566\u2560\u2550\u256c\u00a4\u00f0\u00d0\u00ca\u00cb\u00c8\u0131\u00cd\u00ce\u00cf\u2518\u250c\u2588\u2584\u00a6\u00cc\u2580" +
    "\u00d3\u00df\u00d4\u00d2\u00f5\u00d5\u00b5\u00fe\u00de\u00da\u00db\u00d9\u00fd\u00dd\u00af\u00b4\u00ad\u00b1\u2017\u00be\u00b6\u00a7\u00f7\u00b8\u00b0\u00a8\u00b7\u00b9\u00b3\u00b2\u25a0\u00a0",
  cp852:
    "\u00c7\u00fc\u00e9\u00e2\u00e4\u016f\u0107\u00e7\u0142\u00eb\u0150\u0151\u00ee\u0179\u00c4\u0106\u00c9\u0139\u013a\u00f4\u00f6\u013d\u013e\u015a\u015b\u00d6\u00dc\u0164\u0165\u0141\u00d7\u010d" +
    "\u00e1\u00ed\u00f3\u00fa\u0104\u0105\u017d\u017e\u0118\u0119\u00ac\u017a\u010c\u015f\u00ab\u00bb\u2591\u2592\u2593\u2502\u2524\u00c1\u00c2\u011a\u015e\u2563\u2551\u2557\u255d\u017b\u017c\u2510" +
    "\u2514\u2534\u252c\u251c\u2500\u253c\u0102\u0103\u255a\u2554\u2569\u2566\u2560\u2550\u256c\u00a4\u0111\u0110\u010e\u00cb\u010f\u0147\u00cd\u00ce\u011b\u2518\u250c\u2588\u2584\u0162\u016e\u2580" +
    "\u00d3\u00df\u00d4\u0143\u0144\u0148\u0160\u0161\u0154\u00da\u0155\u0170\u00fd\u00dd\u0163\u00b4\u00ad\u02dd\u02db\u02c7\u02d8\u00a7\u00f7\u00b8\u00b0\u00a8\u02d9\u0171\u0158\u0159\u25a0\u00a0",
  cp857:
    "\u00c7\u00fc\u00e9\u00e2\u00e4\u00e0\u00e5\u00e7\u00ea\u00eb\u00e8\u00ef\u00ee\u0131\u00c4\u00c5\u00c9\u00e6\u00c6\u00f4\u00f6\u00f2\u00fb\u00f9\u0130\u00d6\u00dc\u00f8\u00a3\u00d8\u015e\u015f" +
    "\u00e1\u00ed\u00f3\u00fa\u00f1\u00d1\u011e\u011f\u00bf\u00ae\u00ac\u00bd\u00bc\u00a1\u00ab\u00bb\u2591\u2592\u2593\u2502\u2524\u00c1\u00c2\u00c0\u00a9\u2563\u2551\u2557\u255d\u00a2\u00a5\u2510" +
    "\u2514\u2534\u252c\u251c\u2500\u253c\u00e3\u00c3\u255a\u2554\u2569\u2566\u2560\u2550\u256c\u00a4\u00ba\u00aa\u00ca\u00cb\u00c8\ufffd\u00cd\u00ce\u00cf\u2518\u250c\u2588\u2584\u00a6\u00cc\u2580" +
    "\u00d3\u00df\u00d4\u00d2\u00f5\u00d5\u00b5\ufffd\u00d7\u00da\u00db\u00d9\u00ec\u00ff\u00af\u00b4\u00ad\u00b1\ufffd\u00be\u00b6\u00a7\u00f7\u00b8\u00b0\u00a8\u00b7\u00b9\u00b3\u00b2\u25a0\u00a0",
  cp858:
    "\u00c7\u00fc\u00e9\u00e2\u00e4\u00e0\u00e5\u00e7\u00ea\u00eb\u00e8\u00ef\u00ee\u00ec\u00c4\u00c5\u00c9\u00e6\u00c6\u00f4\u00f6\u00f2\u00fb\u00f9\u00ff\u00d6\u00dc\u00f8\u00a3\u00d8\u00d7\u0192" +
    "\u00e1\u00ed\u00f3\u00fa\u00f1\u00d1\u00aa\u00ba\u00bf\u00ae\u00ac\u00bd\u00bc\u00a1\u00ab\u00bb\u2591\u2592\u2593\u2502\u2524\u00c1\u00c2\u00c0\u00a9\u2563\u2551\u2557\u255d\u00a2\u00a5\u2510" +
    "\u2514\u2534\u252c\u251c\u2500\u253c\u00e3\u00c3\u255a\u2554\u2569\u2566\u2560\u2550\u256c\u00a4\u00f0\u00d0\u00ca\u00cb\u00c8\u20ac\u00cd\u00ce\u00cf\u2518\u250c\u2588\u2584\u00a6\u00cc\u2580" +
    "\u00d3\u00df\u00d4\u00d2\u00f5\u00d5\u00b5\u00fe\u00de\u00da\u00db\u00d9\u00fd\u00dd\u00af\u00b4\u00ad\u00b1\u2017\u00be\u00b6\u00a7\u00f7\u00b8\u00b0\u00a8\u00b7\u00b9\u00b3\u00b2\u25a0\u00a0",
  cp860:
    "\u00c7\u00fc\u00e9\u00e2\u00e3\u00e0\u00c1\u00e7\u00ea\u00ca\u00e8\u00cd\u00d4\u00ec\u00c3\u00c2\u00c9\u00c0\u00c8\u00f4\u00f5\u00f2\u00da\u00f9\u00cc\u00d5\u00dc\u00a2\u00a3\u00d9\u20a7\u00d3" +
    "\u00e1\u00ed\u00f3\u00fa\u00f1\u00d1\u00aa\u00ba\u00bf\u00d2\u00ac\u00bd\u00bc\u00a1\u00ab\u00bb\u2591\u2592\u2593\u2502\u2524\u2561\u2562\u2556\u2555\u2563\u2551\u2557\u255d\u255c\u255b\u2510" +
    "\u2514\u2534\u252c\u251c\u2500\u253c\u255e\u255f\u255a\u2554\u2569\u2566\u2560\u2550\u256c\u2567\u2568\u2564\u2565\u2559\u2558\u2552\u2553\u256b\u256a\u2518\u250c\u2588\u2584\u258c\u2590\u2580" +
    "\u03b1\u00df\u0393\u03c0\u03a3\u03c3\u00b5\u03c4\u03a6\u0398\u03a9\u03b4\u221e\u03c6\u03b5\u2229\u2261\u00b1\u2265\u2264\u2320\u2321\u00f7\u2248\u00b0\u2219\u00b7\u221a\u207f\u00b2\u25a0\u00a0",
  cp863:
    "\u00c7\u00fc\u00e9\u00e2\u00c2\u00e0\u00b6\u00e7\u00ea\u00eb\u00e8\u00ef\u00ee\u2017\u00c0\u00a7\u00c9\u00c8\u00ca\u00f4\u00cb\u00cf\u00fb\u00f9\u00a4\u00d4\u00dc\u00a2\u00a3\u00d9\u00db\u0192" +
    "\u00a6\u00b4\u00f3\u00fa\u00a8\u00b8\u00b3\u00af\u00ce\u2310\u00ac\u00bd\u00bc\u00be\u00ab\u00bb\u2591\u2592\u2593\u2502\u2524\u2561\u2562\u2556\u2555\u2563\u2551\u2557\u255d\u255c\u255b\u2510" +
    "\u2514\u2534\u252c\u251c\u2500\u253c\u255e\u255f\u255a\u2554\u2569\u2566\u2560\u2550\u256c\u2567\u2568\u2564\u2565\u2559\u2558\u2552\u2553\u256b\u256a\u2518\u250c\u2588\u2584\u258c\u2590\u2580" +
    "\u03b1\u00df\u0393\u03c0\u03a3\u03c3\u00b5\u03c4\u03a6\u0398\u03a9\u03b4\u221e\u03c6\u03b5\u2229\u2261\u00b1\u2265\u2264\u2320\u2321\u00f7\u2248\u00b0\u2219\u00b7\u221a\u207f\u00b2\u25a0\u00a0",
  cp865:
    "\u00c7\u00fc\u00e9\u00e2\u00e4\u00e0\u00e5\u00e7\u00ea\u00eb\u00e8\u00ef\u00ee\u00ec\u00c4\u00c5\u00c9\u00e6\u00c6\u00f4\u00f6\u00f2\u00fb\u00f9\u00ff\u00d6\u00dc\u00f8\u00a3\u00d8\u20a7\u0192" +
    "\u00e1\u00ed\u00f3\u00fa\u00f1\u00d1\u00aa\u00ba\u00bf\u2310\u00ac\u00bd\u00bc\u00a1\u00ab\u00a4\u2591\u2592\u2593\u2502\u2524\u2561\u2562\u2556\u2555\u2563\u2551\u2557\u255d\u255c\u255b\u2510" +
    "\u2514\u2534\u252c\u251c\u2500\u253c\u255e\u255f\u255a\u2554\u2569\u2566\u2560\u2550\u256c\u2567\u2568\u2564\u2565\u2559\u2558\u2552\u2553\u256b\u256a\u2518\u250c\u2588\u2584\u258c\u2590\u2580" +
    "\u03b1\u00df\u0393\u03c0\u03a3\u03c3\u00b5\u03c4\u03a6\u0398\u03a9\u03b4\u221e\u03c6\u03b5\u2229\u2261\u00b1\u2265\u2264\u2320\u2321\u00f7\u2248\u00b0\u2219\u00b7\u221a\u207f\u00b2\u25a0\u00a0",
};

/** The QZ `encoding` option: a charset name, or `{ from, to }` to convert bytes. */
export type QzEncoding = { from?: string | null; to?: string | null };

export function parseEncoding(option: unknown): QzEncoding {
  if (option !== null && typeof option === "object") {
    const { from, to } = option as { from?: unknown; to?: unknown };
    return { from: typeof from === "string" ? from : null, to: typeof to === "string" ? to : null };
  }
  return { from: null, to: typeof option === "string" ? option : null };
}

type Charset =
  | { kind: "utf-8" }
  | { kind: "latin-1" }
  | { kind: "table"; name: string; chars: string };

const tableCache = new Map<string, string>();

/** Resolve a Java or WHATWG charset name; null or blank means QZ Tray's default, UTF-8. */
function resolveCharset(name: string | null | undefined): Charset {
  const label = (name ?? "").trim().toLowerCase();
  const compact = label.replace(/[^a-z0-9]/g, "");
  if (compact === "" || compact === "utf8") return { kind: "utf-8" };
  // WHATWG treats "latin1" as windows-1252; Java means true ISO-8859-1.
  if (compact === "iso88591" || compact === "latin1" || compact === "iso8859") return { kind: "latin-1" };

  // "IBM00858", "ibm-858", "Cp858", "858" all name DOS code page 858.
  const dos = /^(?:cp|ibm)?0*(\d{3})$/.exec(compact);
  if (dos) {
    const chars = DOS_CODE_PAGES[`cp${dos[1]}`];
    if (chars !== undefined) return { kind: "table", name: `cp${dos[1]}`, chars };
  }

  const chars = singleByteTable(label);
  if (chars !== undefined) return { kind: "table", name: label, chars };

  throw new EscpostError(
    "UNSUPPORTED_FORMAT",
    `ESCPost does not know the encoding "${name}". Supported: UTF-8, ISO-8859-1, ` +
      `${Object.keys(DOS_CODE_PAGES).join(", ")}, and the single-byte charsets of the browser (windows-1252...).`,
  );
}

/** Upper half of a single-byte charset the browser can decode, such as windows-1252. */
function singleByteTable(label: string): string | undefined {
  if (tableCache.has(label)) return tableCache.get(label);
  let decoder: TextDecoder;
  try {
    decoder = new TextDecoder(label);
  } catch {
    return undefined;
  }
  const high = Uint8Array.from({ length: 128 }, (_, index) => 0x80 + index);
  const chars = decoder.decode(high);
  // A multi-byte charset (Shift_JIS, GBK...) would not decode 128 bytes to 128 characters.
  if (chars.length !== 128) return undefined;
  tableCache.set(label, chars);
  return chars;
}

/** Encode text like Java's `String.getBytes(charset)`. */
export function encodeText(text: string, charsetName: string | null | undefined): Uint8Array {
  const charset = resolveCharset(charsetName);
  if (charset.kind === "utf-8") return new TextEncoder().encode(text);

  const bytes: number[] = [];
  for (const char of text) {
    const code = char.codePointAt(0)!;
    if (code < 0x80) {
      bytes.push(code);
    } else if (charset.kind === "latin-1") {
      bytes.push(code <= 0xff ? code : UNMAPPABLE);
    } else {
      const index = charset.chars.indexOf(char);
      bytes.push(index === -1 || char === "�" ? UNMAPPABLE : 0x80 + index);
    }
  }
  return Uint8Array.from(bytes);
}

/** Decode bytes in a charset, for the `{ from, to }` form of the option. */
export function decodeBytes(bytes: Uint8Array, charsetName: string): string {
  const charset = resolveCharset(charsetName);
  if (charset.kind === "utf-8") return new TextDecoder().decode(bytes);
  let text = "";
  for (const byte of bytes) {
    text += byte < 0x80 || charset.kind === "latin-1" ? String.fromCharCode(byte) : charset.chars[byte - 0x80];
  }
  return text;
}
