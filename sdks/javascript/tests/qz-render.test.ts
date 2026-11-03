import { describe, expect, it } from "vitest";
import { assertEscposLanguage, printableWidth, rasterToEscpos, renderElements } from "../src/qz/render";

/** RGBA pixels from rows of "#" (black) and "." (white). */
function pixels(rows: string[]): Uint8ClampedArray {
  const values = rows.flatMap((row) =>
    Array.from(row).flatMap((cell) => (cell === "#" ? [0, 0, 0, 255] : [255, 255, 255, 255])),
  );
  return Uint8ClampedArray.from(values);
}

function countBits(byte: number): number {
  let count = 0;
  for (let bit = 0; bit < 8; bit++) count += (byte >> bit) & 1;
  return count;
}

describe("rasterToEscpos", () => {
  it("packs each row 8 pixels per byte under one GS v 0 header", () => {
    const bytes = rasterToEscpos(pixels(["#.......", "........", "#######."]), 8, 3);
    expect(Array.from(bytes)).toEqual([0x1d, 0x76, 0x30, 0x00, 1, 0, 3, 0, 0x80, 0x00, 0xfe]);
  });

  it("pads a row that is not a whole number of bytes with white", () => {
    const bytes = rasterToEscpos(pixels(["#########"]), 9, 1);
    expect(Array.from(bytes)).toEqual([0x1d, 0x76, 0x30, 0x00, 2, 0, 1, 0, 0xff, 0x80]);
  });

  it("dithers a mid-grey area into a bit less than half black dots, to make up for dot gain", () => {
    const size = 32;
    const bytes = rasterToEscpos(new Uint8ClampedArray(size * size * 4).fill(128), size, size);
    const ones = Array.from(bytes.slice(8)).reduce((sum, byte) => sum + countBits(byte), 0);
    // Grey 128 is lightened to about 147 before dithering: about 42% black dots, not 50%.
    expect(ones / (size * size)).toBeGreaterThan(0.37);
    expect(ones / (size * size)).toBeLessThan(0.47);
  });

  it("leaves pure black and pure white exactly as they are", () => {
    const black = rasterToEscpos(pixels(["########", "########"]), 8, 2);
    const white = rasterToEscpos(pixels(["........", "........"]), 8, 2);
    expect(Array.from(black.slice(8))).toEqual([0xff, 0xff]);
    expect(Array.from(white.slice(8))).toEqual([0x00, 0x00]);
  });

  it("keeps a light grey line visible as sparse dots, where a plain threshold printed nothing", () => {
    const width = 64;
    const bytes = rasterToEscpos(new Uint8ClampedArray(width * 4 * 4).fill(200), width, 4);
    const ones = Array.from(bytes.slice(8)).reduce((sum, byte) => sum + countBits(byte), 0);
    expect(ones).toBeGreaterThan(0);
  });

  it("splits a tall image into bands of at most 256 rows", () => {
    const height = 300;
    const bytes = rasterToEscpos(new Uint8ClampedArray(8 * height * 4).fill(255), 8, height);
    expect(Array.from(bytes.slice(0, 8))).toEqual([0x1d, 0x76, 0x30, 0x00, 1, 0, 0, 1]);
    const second = 8 + 256;
    expect(Array.from(bytes.slice(second, second + 8))).toEqual([0x1d, 0x76, 0x30, 0x00, 1, 0, 44, 0]);
    expect(bytes.length).toBe(8 + 256 + 8 + 44);
  });
});

describe("printer language of raw image, HTML and PDF jobs", () => {
  it("accepts ESC/POS under QZ's names, or no language at all", () => {
    for (const language of [undefined, "", "ESCPOS", "escpos", "ESC/POS"]) {
      expect(() => assertEscposLanguage({ language })).not.toThrow();
    }
  });

  it("refuses the label printer languages ESCPost cannot draw in", () => {
    for (const language of ["ZPL", "EPL", "CPCL", "SBPL", "EVOLIS", "PGL"]) {
      expect(() => assertEscposLanguage({ language })).toThrow(
        expect.objectContaining({ code: "UNSUPPORTED_FORMAT", message: expect.stringContaining(language) }),
      );
    }
  });

  it("refuses a ZPL image job before drawing it, instead of sending ESC/POS bytes", async () => {
    const job = { type: "raw", format: "image", flavor: "base64", data: "iVBORw0KGgo=", options: { language: "ZPL" } };
    await expect(renderElements([job], {})).rejects.toMatchObject({
      code: "UNSUPPORTED_FORMAT",
      message: expect.stringContaining("ZPL"),
    });
  });
});

describe("printableWidth", () => {
  it("is 72 mm (576 dots) when the page gives no paper size, the usual 80 mm roll", () => {
    expect(printableWidth({})).toBe(576);
    expect(printableWidth({ size: null })).toBe(576);
  });

  it("follows the paper size the page gives, minus the print head's margins", () => {
    expect(printableWidth({ size: { width: 58 }, units: "mm" })).toBe(384);
    expect(printableWidth({ size: { width: 2.25 } })).toBe(384); // inches, QZ's default unit
    expect(printableWidth({ size: { width: 8 }, units: "cm" })).toBe(576);
    expect(printableWidth({ size: { width: 3.15 } })).toBe(576);
    expect(printableWidth({ size: { width: 104 }, units: "mm" })).toBe(767); // 96 mm printable
  });
});
