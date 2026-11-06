/**
 * QZ Tray's own way of turning a raw image into ESC/POS, for pages that ask for
 * it with the raw image options. Ported from QZ Tray's `MonoImageConverter`,
 * `EscPos`, `EscAsterisk`, `GsV0` and `GsL` (github.com/qzind/tray), so the same
 * options give the same bytes.
 *
 * A raw image job that sets none of these options keeps ESCPost's own encoding
 * instead (dithered `GS v 0`, see `rasterToEscpos` in render.ts).
 */

export type QzImageOptions = {
  encoding: "esc_asterisk" | "gs_v_0" | "gs_l";
  /** The `m` of `ESC * m`: 32 single density, 33 double, 39 triple, or any number the page gives. */
  dotDensity: number;
  /** QZ's "-legacy" densities: line spacing commands and line feeds around `ESC *`. */
  legacy: boolean;
  /** "dither" is ours: QZ Tray lists it but cannot print it. */
  quantization: "luma" | "alpha" | "black" | "dither";
  threshold: number;
};

const OPTION_NAMES = ["imageEncoding", "dotDensity", "quantization", "threshold"] as const;
/** The rows each `GS v 0` or `GS ( L` command carries in QZ Tray. */
const QZ_SLICE_HEIGHT = 24;

/** QZ Tray treats a blank option as unset, as its own demo page sends them. */
function given(value: unknown): value is string | number {
  return value !== undefined && value !== null && String(value).trim() !== "";
}

/**
 * The page's raw image options, with QZ Tray's defaults for the ones it leaves
 * out, or undefined when it sets none of them.
 */
export function qzImageOptions(options: Record<string, unknown>): QzImageOptions | undefined {
  if (!OPTION_NAMES.some((name) => given(options[name]))) return undefined;

  const encoding = String(options["imageEncoding"] ?? "").toLowerCase();
  const quantization = String(options["quantization"] ?? "").toLowerCase();
  const density = parseDotDensity(options["dotDensity"]);
  const threshold = Number.parseInt(String(options["threshold"] ?? ""), 10);

  return {
    encoding: encoding === "gs_v_0" || encoding === "gs_l" ? encoding : "esc_asterisk",
    dotDensity: Math.abs(density),
    legacy: density < 0,
    quantization: quantization === "alpha" || quantization === "black" || quantization === "dither" ? quantization : "luma",
    threshold: Number.isNaN(threshold) ? 127 : threshold,
  };
}

/**
 * How many dots wide each image column prints. `ESC *` single densities (m = 0
 * and 32) halve the horizontal resolution, so every column prints two dots
 * wide: that is why QZ Tray's default raw image comes out twice as wide.
 */
export function horizontalStretch(options: QzImageOptions): number {
  return options.encoding === "esc_asterisk" && (options.dotDensity === 0 || options.dotDensity === 32) ? 2 : 1;
}

/** QZ Tray's names for `ESC *` densities; negative means legacy mode. */
function parseDotDensity(value: unknown): number {
  const text = String(value ?? "").trim().toLowerCase();
  const named: Record<string, number> = {
    single: 32,
    double: 33,
    triple: 39,
    "single-legacy": -32,
    "double-legacy": -33,
  };
  if (text in named) return named[text]!;
  const number = Number.parseInt(text, 10);
  return Number.isNaN(number) ? 32 : number;
}

/**
 * RGBA pixels to ESC/POS, the way QZ Tray does it: pad the width to a multiple
 * of 8, decide each pixel black or white, then encode with the chosen command.
 * `dither` pixels come from the caller, which owns the dithering.
 */
export function qzImageToEscpos(
  rgba: ArrayLike<number>,
  width: number,
  height: number,
  options: QzImageOptions,
  dither: (rgba: ArrayLike<number>, width: number, height: number) => Uint8Array,
): Uint8Array {
  const paddedWidth = Math.ceil(width / 8) * 8;
  const padded = padRight(rgba, width, height, paddedWidth);
  const black =
    options.quantization === "dither"
      ? dither(whiteBehind(padded), paddedWidth, height)
      : quantize(padded, options.quantization, options.threshold);

  switch (options.encoding) {
    case "gs_v_0":
      return gsV0(black, paddedWidth, height);
    case "gs_l":
      return gsL(black, paddedWidth, height);
    default:
      return escAsterisk(black, paddedWidth, height, options.dotDensity, options.legacy);
  }
}

/** QZ Tray pads the right side with transparent pixels, which every rule reads as white. */
export function padRight(rgba: ArrayLike<number>, width: number, height: number, paddedWidth: number): Uint8ClampedArray {
  const padded = new Uint8ClampedArray(paddedWidth * height * 4);
  for (let y = 0; y < height; y++) {
    for (let index = 0; index < width * 4; index++) {
      padded[(y * paddedWidth) * 4 + index] = rgba[(y * width) * 4 + index]!;
    }
  }
  return padded;
}

/** Transparent pixels as paper, for the dithering, which only reads colours. */
export function whiteBehind(rgba: Uint8ClampedArray): Uint8ClampedArray {
  const flat = new Uint8ClampedArray(rgba.length);
  for (let offset = 0; offset < rgba.length; offset += 4) {
    const alpha = rgba[offset + 3]! / 255;
    for (let channel = 0; channel < 3; channel++) {
      flat[offset + channel] = rgba[offset + channel]! * alpha + 255 * (1 - alpha);
    }
    flat[offset + 3] = 255;
  }
  return flat;
}

/** QZ Tray's `MonoImageConverter.isBlack`, one entry per pixel, 1 for black. */
export function quantize(rgba: ArrayLike<number>, rule: "luma" | "alpha" | "black", threshold: number): Uint8Array {
  const black = new Uint8Array(rgba.length / 4);
  for (let index = 0; index < black.length; index++) {
    const [r, g, b, a] = [rgba[index * 4]!, rgba[index * 4 + 1]!, rgba[index * 4 + 2]!, rgba[index * 4 + 3]!];
    let isBlack: boolean;
    if (rule === "alpha") {
      isBlack = a > threshold;
    } else if (rule === "black") {
      isBlack = r === 0 && g === 0 && b === 0 && a === 255;
    } else {
      // Pixels less opaque than the threshold are white, then the luma decides.
      isBlack = a >= threshold && Math.trunc((r * 299 + g * 587 + b * 114) / 1000) < threshold;
    }
    black[index] = isBlack ? 1 : 0;
  }
  return black;
}

/** QZ Tray's `EscAsterisk`: column images, 24 rows (or 8) per `ESC *` command. */
export function escAsterisk(black: Uint8Array, width: number, height: number, density: number, legacy: boolean): Uint8Array {
  const out: number[] = [];
  const segmentHeight = density > 1 ? 24 : density === 1 ? 8 : 16;
  // Impact printers print density 1 in two passes, odd rows then even rows.
  const stripe = density === 1;
  const bytesNeeded = density <= 1 ? 1 : 3;

  if (legacy) out.push(0x1b, 0x33, 24);

  let offset = 0;
  let zeroPass = true;
  while (offset < height) {
    out.push(0x1b, 0x2a, density & 0xff, width % 256, Math.floor(width / 256) & 0xff);
    for (let x = 0; x < width; x++) {
      for (let bite = 0; bite < bytesNeeded; bite++) {
        let slice = 0;
        const first = zeroPass ? 0 : 1;
        const step = stripe ? 2 : 1;
        for (let bit = first; bit < 8 * step; bit += step) {
          const y = offset + bite * 8 + bit;
          const index = y * width + x;
          if (index < black.length && black[index] === 1) slice |= 1 << (7 - Math.trunc((bit - first) / step));
        }
        out.push(slice & 0xff);
      }
    }

    if (stripe) {
      if (zeroPass) {
        out.push(0x1b, 0x4a, 0x01);
      } else {
        out.push(0x1b, 0x4a, segmentHeight - 1);
        offset += 8 * bytesNeeded;
      }
      zeroPass = !zeroPass;
    } else {
      if (legacy) out.push(0x0a);
      else out.push(0x1b, 0x4a, segmentHeight);
      offset += 8 * bytesNeeded;
    }
  }

  if (legacy) out.push(0x1b, 0x33, 30);
  return Uint8Array.from(out);
}

/** Rows `top` to `top + rows`, packed 8 pixels per byte, most significant bit first. */
function packRows(black: Uint8Array, width: number, top: number, rows: number): number[] {
  const bytes: number[] = [];
  for (let start = top * width; start < (top + rows) * width; start += 8) {
    let byte = 0;
    for (let bit = 0; bit < 8; bit++) if (black[start + bit] === 1) byte |= 0x80 >> bit;
    bytes.push(byte);
  }
  return bytes;
}

/** QZ Tray's `GsV0`: one `GS v 0` command per 24 rows. */
export function gsV0(black: Uint8Array, width: number, height: number): Uint8Array {
  const out: number[] = [];
  const bytesPerRow = Math.ceil(width / 8);
  for (let top = 0; top < height; top += QZ_SLICE_HEIGHT) {
    const rows = Math.min(QZ_SLICE_HEIGHT, height - top);
    out.push(0x1d, 0x76, 0x30, 0x00, bytesPerRow & 0xff, (bytesPerRow >> 8) & 0xff, rows & 0xff, (rows >> 8) & 0xff);
    out.push(...packRows(black, width, top, rows));
  }
  return Uint8Array.from(out);
}

/** QZ Tray's `GsL`: per 24 rows, store the slice (`GS ( L` fn 112) then print it (fn 50). */
export function gsL(black: Uint8Array, width: number, height: number): Uint8Array {
  const out: number[] = [];
  for (let top = 0; top < height; top += QZ_SLICE_HEIGHT) {
    const rows = Math.min(QZ_SLICE_HEIGHT, height - top);
    const data = packRows(black, width, top, rows);
    const length = data.length + 10;
    out.push(0x1d, 0x28, 0x4c, length & 0xff, (length >> 8) & 0xff, 48, 112, 48, 1, 1, 49);
    out.push(width & 0xff, (width >> 8) & 0xff, rows & 0xff, (rows >> 8) & 0xff, ...data);
    out.push(0x1d, 0x28, 0x4c, 2, 0, 48, 50);
  }
  return Uint8Array.from(out);
}
