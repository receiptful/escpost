/**
 * QZ Tray's raw image encoders for label printer languages: ZPL, EPL, CPCL and
 * SBPL. Ported from QZ Tray's `Zpl`, `Epl`, `Cpcl`, `Sbpl`, `MonoImageConverter`
 * and `LanguageType` (github.com/qzind/tray), so a page that prints a label
 * image, HTML page or PDF through QZ Tray gets the same bytes from ESCPost.
 *
 * Like QZ Tray, only the image command is produced. The page wraps it in its
 * own label commands, for example `^XA^FO50,50` before and `^FS^XZ` after in
 * ZPL, or `N` before and `P1,1` after in EPL.
 */

import { EscpostError } from "../errors";
import { padRight, quantize, whiteBehind } from "./escpos-image";

export type LabelLanguage = "zpl" | "epl" | "cpcl" | "sbpl";
export type PrinterLanguage = "escpos" | LabelLanguage;

/**
 * QZ Tray's language names. Like QZ, a name matches case-insensitively and an
 * alternative name exactly; ESCPost also accepts alternative names in any case.
 */
const LANGUAGE_NAMES: Record<PrinterLanguage, string[]> = {
  escpos: ["ESCPOS", "ESC/POS"],
  zpl: ["ZPL", "ZEBRA", "ZPL2", "ZPLII"],
  epl: ["EPL", "ELTRON", "EPL2", "EPLII"],
  cpcl: ["CPCL", "COMTEC"],
  sbpl: ["SBPL", "SATO"],
};

/** The dot density QZ Tray assumes for each label language (`LanguageType`). */
export const LABEL_DPI = 203;

export type LabelImageOptions = {
  /** QZ's `quantization`: how a pixel is decided black. "dither" is ours. */
  quantization: "luma" | "alpha" | "black" | "dither";
  threshold: number;
  /** EPL and CPCL only: where the image starts on the label, in dots. */
  x: number;
  y: number;
};

/**
 * The printer language a raw image, HTML or PDF job asks for. No language means
 * ESC/POS. A language QZ Tray knows but ESCPost cannot draw in (Evolis, PGL,
 * DPL...) is refused before anything is drawn, instead of sending the printer
 * commands it cannot read.
 */
export function printerLanguage(value: unknown): PrinterLanguage {
  if (value === undefined || value === null || String(value).trim() === "") return "escpos";
  const name = String(value).trim().toUpperCase();
  for (const [language, names] of Object.entries(LANGUAGE_NAMES) as [PrinterLanguage, string[]][]) {
    if (names.includes(name)) return language;
  }
  throw new EscpostError(
    "UNSUPPORTED_FORMAT",
    `ESCPost draws images, HTML and PDFs in ESC/POS, ZPL, EPL, CPCL or SBPL, not in the "${String(value).trim()}" printer language.`,
  );
}

/** QZ Tray's options for a label image, with its defaults. */
export function labelImageOptions(options: Record<string, unknown>): LabelImageOptions {
  const quantization = String(options["quantization"] ?? "").toLowerCase();
  return {
    quantization: quantization === "alpha" || quantization === "black" || quantization === "dither" ? quantization : "luma",
    threshold: integer(options["threshold"], 127),
    x: integer(options["x"], 0),
    y: integer(options["y"], 0),
  };
}

/**
 * One image in a label printer language, from RGBA pixels. As in QZ Tray, the
 * width is padded on the right to a multiple of 8 with white, each pixel is
 * decided black or white (EPL counts 0 as black), and every 8 pixels of a row
 * make one byte, leftmost pixel in the highest bit.
 */
export function rasterToLabel(
  rgba: ArrayLike<number>,
  width: number,
  height: number,
  language: LabelLanguage,
  options: LabelImageOptions,
  dither: (rgba: ArrayLike<number>, width: number, height: number) => Uint8Array,
): Uint8Array {
  const paddedWidth = Math.ceil(width / 8) * 8;
  const padded = padRight(rgba, width, height, paddedWidth);
  const black =
    options.quantization === "dither"
      ? dither(whiteBehind(padded), paddedWidth, height)
      : quantize(padded, options.quantization, options.threshold);
  const bytes = packBits(black, language === "epl");
  const perRow = paddedWidth / 8;

  switch (language) {
    case "zpl":
      // ^GFA,<total bytes>,<total bytes>,<bytes per row>,<hex>
      return ascii(`^GFA,${bytes.length},${bytes.length},${perRow},${hex(bytes)}`);
    case "epl":
      // GW<x>,<y>,<bytes per row>,<rows>,<binary>LF
      return concat(ascii(`GW${options.x},${options.y},${perRow},${height},`), bytes, ascii("\n"));
    case "cpcl":
      // EG <bytes per row> <rows> <x> <y> <hex>CR LF
      return ascii(`EG ${perRow} ${height} ${options.x} ${options.y} ${hex(bytes)}\r\n`);
    case "sbpl":
      // ESC G H <bytes per row, 3 digits> <rows / 8, 3 digits> <hex>
      return ascii(`\x1bGH${pad3(perRow)}${pad3(Math.floor(height / 8))}${hex(bytes)}`);
  }
}

/** QZ Tray's `ByteUtilities.toByteArray(PixelGrid)`: 8 pixels per byte, first pixel in bit 7. */
function packBits(black: Uint8Array, invert: boolean): Uint8Array {
  const bytes = new Uint8Array(Math.floor(black.length / 8));
  for (let index = 0; index < bytes.length; index++) {
    let byte = 0;
    for (let bit = 0; bit < 8; bit++) {
      if ((black[index * 8 + bit] === 1) !== invert) byte |= 0x80 >> bit;
    }
    bytes[index] = byte;
  }
  return bytes;
}

/** Upper-case hexadecimal, as QZ Tray's `ByteUtilities.toHexString`. */
function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).toUpperCase().padStart(2, "0")).join("");
}

function ascii(text: string): Uint8Array {
  return Uint8Array.from(text, (character) => character.charCodeAt(0));
}

function concat(...parts: Uint8Array[]): Uint8Array {
  const joined = new Uint8Array(parts.reduce((length, part) => length + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    joined.set(part, offset);
    offset += part.length;
  }
  return joined;
}

function pad3(value: number): string {
  return String(value).padStart(3, "0");
}

/** QZ Tray's `optInt`: a number, or the default when the option is missing or not a number. */
function integer(value: unknown, fallback: number): number {
  const number = Number.parseInt(String(value ?? ""), 10);
  return Number.isNaN(number) ? fallback : number;
}
