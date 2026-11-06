import { describe, expect, it } from "vitest";
import {
  assertEscposLanguage,
  imageDrawWidth,
  pdfScale,
  pixelDotsPerCssPixel,
  printableWidth,
  printerRows,
  rasterToEscpos,
  renderElements,
  targetWidth,
} from "../src/qz/render";

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

  it("prints a thin anti-aliased stroke as a solid line, as a printer driver draws text", () => {
    // Small text drawn by the browser is mostly grey edges: here a stroke a
    // little thinner than one dot, so it comes out grey 150, not black.
    const size = 16;
    const rgba = new Uint8ClampedArray(size * size * 4).fill(255);
    for (let y = 0; y < size; y++) rgba.fill(150, (y * size + 8) * 4, (y * size + 8) * 4 + 3);
    const bytes = rasterToEscpos(rgba, size, size);
    for (let y = 0; y < size; y++) {
      const row = Array.from(bytes.slice(8 + y * 2, 8 + y * 2 + 2));
      expect(row).toEqual([0x00, 0x80]);
    }
  });

  it("keeps a large grey shape shaded inside, with a solid outline", () => {
    const size = 40;
    const rgba = new Uint8ClampedArray(size * size * 4).fill(255);
    for (let y = 4; y < 36; y++) rgba.fill(128, (y * size + 4) * 4, (y * size + 36) * 4);
    const bytes = rasterToEscpos(rgba, size, size);
    const bytesPerRow = size / 8;
    const black = (x: number, y: number) => (bytes[8 + y * bytesPerRow + (x >> 3)]! >> (7 - (x & 7))) & 1;
    // The outline is a sharp edge: solid black.
    for (let y = 4; y < 36; y++) expect(black(4, y)).toBe(1);
    // Away from the edge the grey is still dithered, not filled.
    let ones = 0;
    for (let y = 8; y < 32; y++) for (let x = 8; x < 32; x++) ones += black(x, y);
    expect(ones / (24 * 24)).toBeGreaterThan(0.3);
    expect(ones / (24 * 24)).toBeLessThan(0.55);
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
  it("is the printer profile's printable width, whatever paper size the page gives", () => {
    const tmT88iii = { printableWidthDots: 512, dpiX: 180, dpiY: 180 };
    expect(printableWidth({}, tmT88iii)).toBe(512);
    expect(printableWidth({ size: { width: 80 }, units: "mm" }, tmT88iii)).toBe(512);
  });

  it("is 384 dots (58 mm at 203 dpi) without a profile or a paper size, ESCPost's safe default", () => {
    expect(printableWidth({})).toBe(384);
    expect(printableWidth({ size: null })).toBe(384);
  });

  it("follows the paper size the page gives when the printer has no profile", () => {
    expect(printableWidth({ size: { width: 58 }, units: "mm" })).toBe(384);
    expect(printableWidth({ size: { width: 2.25 } })).toBe(384); // inches, QZ's default unit
    expect(printableWidth({ size: { width: 8 }, units: "cm" })).toBe(576);
    expect(printableWidth({ size: { width: 3.15 } })).toBe(576);
    expect(printableWidth({ size: { width: 104 }, units: "mm" })).toBe(767); // 96 mm printable
  });
});

describe("sizing a job for the printer's profile", () => {
  const profiles = [
    { printableWidthDots: 384, dpiX: 203, dpiY: 203 },
    { printableWidthDots: 512, dpiX: 180, dpiY: 180 },
    { printableWidthDots: 576, dpiX: 203, dpiY: 203 },
  ];

  for (const profile of profiles) {
    const name = `${profile.printableWidthDots} dots at ${profile.dpiX} dpi`;
    const width = profile.printableWidthDots;

    it(`fills ${name} with a pixel image when scaleContent is on, small or large`, () => {
      expect(imageDrawWidth(targetWidth("pixel", {}, {}, profile), 300, true)).toBe(width);
      expect(imageDrawWidth(targetWidth("pixel", {}, {}, profile), 1000, true)).toBe(width);
    });

    it(`prints an image one pixel per dot on ${name} when scaleContent is off, shrinking only what is too wide`, () => {
      expect(imageDrawWidth(targetWidth("pixel", {}, { scaleContent: false }, profile), 300, false)).toBe(300);
      expect(imageDrawWidth(targetWidth("pixel", {}, { scaleContent: false }, profile), 1000, false)).toBe(width);
    });

    it(`fits a PDF page to ${name} when scaleContent is on`, () => {
      const pageWidth = 141.73; // 50 mm in points
      expect(Math.round(pageWidth * pdfScale(width, pageWidth, true, profile))).toBe(width);
    });

    it(`keeps a PDF page's real size at ${profile.dpiX} dpi when scaleContent is off, capped to ${width} dots`, () => {
      const pageWidth = 141.73; // 50 mm in points: 400 dots at 203 dpi, 354 at 180 dpi
      const realSize = Math.round((50 / 25.4) * profile.dpiX);
      expect(Math.round(pageWidth * pdfScale(width, pageWidth, false, profile))).toBe(Math.min(width, realSize));
    });

    it(`draws pixel HTML at ${profile.dpiX} dots per inch of CSS on ${name}`, () => {
      expect(pixelDotsPerCssPixel(profile)).toBeCloseTo(profile.dpiX / 96);
    });

    it(`converts a pixel job's pageWidth at ${profile.dpiX} dpi, capped to ${width} dots`, () => {
      expect(targetWidth("pixel", { pageWidth: 2 }, {}, profile)).toBe(Math.min(width, 2 * profile.dpiX));
    });
  }

  it("uses 203 dpi for a printer without a profile", () => {
    expect(pixelDotsPerCssPixel(undefined)).toBeCloseTo(203 / 96);
    expect(pdfScale(384, 113.39, false, undefined)).toBeCloseTo(203 / 72); // a 40 mm page: 320 dots, under the cap
  });

  it("stretches a pixel job's rows by dpiY / dpiX, so a printer with oblong dots keeps the proportions", () => {
    expect(printerRows(100, { printableWidthDots: 384, dpiX: 203, dpiY: 203 })).toBe(100);
    expect(printerRows(100, { printableWidthDots: 384, dpiX: 180, dpiY: 360 })).toBe(200);
    expect(printerRows(100, undefined)).toBe(100);
  });
});
