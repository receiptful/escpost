import { describe, expect, it } from "vitest";
import { escAsterisk, gsL, gsV0, horizontalStretch, qzImageOptions, qzImageToEscpos, quantize } from "../src/qz/escpos-image";

const BLACK = [0, 0, 0, 255];
const WHITE = [255, 255, 255, 255];

/** RGBA pixels from rows of "#" (black) and "." (white). */
function pixels(rows: string[]): Uint8ClampedArray {
  return Uint8ClampedArray.from(rows.flatMap((row) => Array.from(row).flatMap((cell) => (cell === "#" ? BLACK : WHITE))));
}

const neverDither = () => {
  throw new Error("not expected to dither");
};

describe("qzImageOptions", () => {
  it("is undefined when the page sets none, or sends them blank like QZ's demo", () => {
    expect(qzImageOptions({})).toBeUndefined();
    expect(qzImageOptions({ quantization: "", threshold: "", dotDensity: "", imageEncoding: "" })).toBeUndefined();
  });

  it("fills the options the page leaves out with QZ Tray's defaults", () => {
    expect(qzImageOptions({ dotDensity: "double" })).toEqual({
      encoding: "esc_asterisk",
      dotDensity: 33,
      legacy: false,
      quantization: "luma",
      threshold: 127,
    });
  });

  it("reads QZ Tray's names and numbers", () => {
    expect(qzImageOptions({ dotDensity: "single-legacy" })).toMatchObject({ dotDensity: 32, legacy: true });
    expect(qzImageOptions({ dotDensity: "39" })).toMatchObject({ dotDensity: 39 });
    expect(qzImageOptions({ imageEncoding: "GS_V_0" })).toMatchObject({ encoding: "gs_v_0" });
    expect(qzImageOptions({ threshold: "100", quantization: "ALPHA" })).toMatchObject({ threshold: 100, quantization: "alpha" });
  });
});

describe("quantize", () => {
  it("uses luma below the threshold, and treats faint pixels as paper (QZ's default)", () => {
    const rgba = [...BLACK, ...WHITE, 130, 130, 130, 255, 120, 120, 120, 255, 0, 0, 0, 0];
    expect(Array.from(quantize(rgba, "luma", 127))).toEqual([1, 0, 0, 1, 0]);
  });

  it("uses opacity alone with the alpha rule, and exact black with the black rule", () => {
    expect(Array.from(quantize([255, 255, 255, 200, 0, 0, 0, 100], "alpha", 127))).toEqual([1, 0]);
    expect(Array.from(quantize([...BLACK, 1, 0, 0, 255], "black", 127))).toEqual([1, 0]);
  });
});

describe("QZ Tray's ESC/POS image commands", () => {
  it("ESC * double density: 24 rows per command, 3 bytes per column, then feed 24 dots", () => {
    const black = Uint8Array.from([1, 0, 0, 0, 0, 0, 0, 0]); // one row, 8 dots, the first black
    const bytes = Array.from(escAsterisk(black, 8, 1, 33, false));
    expect(bytes.slice(0, 5)).toEqual([0x1b, 0x2a, 33, 8, 0]);
    expect(bytes.slice(5, 8)).toEqual([0x80, 0, 0]); // first column: top dot
    expect(bytes.slice(8, 29).every((byte) => byte === 0)).toBe(true);
    expect(bytes.slice(29)).toEqual([0x1b, 0x4a, 24]);
  });

  it("ESC * legacy mode sets line spacing and feeds with a line feed", () => {
    const bytes = Array.from(escAsterisk(new Uint8Array(8), 8, 1, 32, true));
    expect(bytes.slice(0, 3)).toEqual([0x1b, 0x33, 24]);
    expect(bytes.slice(-4)).toEqual([0x0a, 0x1b, 0x33, 30]);
  });

  it("GS v 0 packs rows 8 dots per byte, 24 rows per command", () => {
    const black = Uint8Array.from([1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    expect(Array.from(gsV0(black, 8, 2))).toEqual([0x1d, 0x76, 0x30, 0, 1, 0, 2, 0, 0xfe, 0x01]);
    const tall = gsV0(new Uint8Array(8 * 30), 8, 30);
    expect(Array.from(tall.slice(0, 8))).toEqual([0x1d, 0x76, 0x30, 0, 1, 0, 24, 0]);
    expect(Array.from(tall.slice(8 + 24, 8 + 24 + 8))).toEqual([0x1d, 0x76, 0x30, 0, 1, 0, 6, 0]);
  });

  it("GS ( L stores each slice (fn 112) and prints it (fn 50)", () => {
    const bytes = Array.from(gsL(Uint8Array.from([1, 0, 0, 0, 0, 0, 0, 0]), 8, 1));
    expect(bytes).toEqual([
      0x1d, 0x28, 0x4c, 11, 0, 48, 112, 48, 1, 1, 49, 8, 0, 1, 0, 0x80,
      0x1d, 0x28, 0x4c, 2, 0, 48, 50,
    ]);
  });

  it("pads the width to a multiple of 8, as QZ Tray does", () => {
    const options = qzImageOptions({ dotDensity: "double" })!;
    const bytes = qzImageToEscpos(pixels(["#########"]), 9, 1, options, neverDither);
    expect(Array.from(bytes.slice(0, 5))).toEqual([0x1b, 0x2a, 33, 16, 0]);
  });
});

describe("horizontalStretch", () => {
  it("is 2 for ESC * single densities, which print every column two dots wide", () => {
    expect(horizontalStretch(qzImageOptions({ dotDensity: "single" })!)).toBe(2);
    expect(horizontalStretch(qzImageOptions({ dotDensity: "single-legacy" })!)).toBe(2);
    expect(horizontalStretch(qzImageOptions({ dotDensity: "0" })!)).toBe(2);
  });

  it("is 1 for double density and for the raster commands", () => {
    expect(horizontalStretch(qzImageOptions({ dotDensity: "double" })!)).toBe(1);
    expect(horizontalStretch(qzImageOptions({ dotDensity: "single", imageEncoding: "gs_v_0" })!)).toBe(1);
  });
});
