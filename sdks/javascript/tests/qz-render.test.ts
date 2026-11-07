import { afterEach, describe, expect, it, vi } from "vitest";
import { jobToBytes } from "../src/qz/jobs";
import { labelImageOptions, printerLanguage, rasterToLabel } from "../src/qz/label-image";
import {
  imageDrawWidth,
  labelPdfScale,
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
  it("takes ESC/POS under QZ's names, or no language at all", () => {
    for (const language of [undefined, "", "ESCPOS", "escpos", "ESC/POS"]) {
      expect(printerLanguage(language)).toBe("escpos");
    }
  });

  it("knows QZ Tray's label printer languages under all their names", () => {
    expect(["ZPL", "zpl", "ZEBRA", "ZPL2", "ZPLII"].map(printerLanguage)).toEqual(Array(5).fill("zpl"));
    expect(["EPL", "ELTRON", "EPL2", "EPLII"].map(printerLanguage)).toEqual(Array(4).fill("epl"));
    expect(["CPCL", "COMTEC"].map(printerLanguage)).toEqual(["cpcl", "cpcl"]);
    expect(["SBPL", "SATO"].map(printerLanguage)).toEqual(["sbpl", "sbpl"]);
  });

  it("refuses the printer languages ESCPost cannot draw in", () => {
    for (const language of ["EVOLIS", "PGL", "DPL", "FGL"]) {
      expect(() => printerLanguage(language)).toThrow(
        expect.objectContaining({ code: "UNSUPPORTED_FORMAT", message: expect.stringContaining(language) }),
      );
    }
  });

  it("refuses an Evolis image job before drawing it, instead of sending commands the printer cannot read", async () => {
    const job = { type: "raw", format: "image", flavor: "base64", data: "iVBORw0KGgo=", options: { language: "EVOLIS" } };
    await expect(renderElements([job], {})).rejects.toMatchObject({
      code: "UNSUPPORTED_FORMAT",
      message: expect.stringContaining("EVOLIS"),
    });
  });
});

describe("labelPdfScale, QZ Tray's raw PDF size for a label", () => {
  it("prints one dot per point when the job gives no size", () => {
    expect(labelPdfScale(240, 120)).toBe(1);
  });

  it("scales the page to the pageWidth in dots", () => {
    expect(labelPdfScale(240, 120, 480)).toBe(2);
  });

  it("scales to the pageHeight when it is the only size, or when the page is taller than the box", () => {
    expect(labelPdfScale(240, 120, undefined, 60)).toBe(0.5);
    expect(labelPdfScale(240, 120, 480, 100)).toBeCloseTo(100 / 120);
    expect(labelPdfScale(240, 120, 480, 480)).toBe(2);
  });
});

describe("rasterToLabel, QZ Tray's label image commands", () => {
  const text = (bytes: Uint8Array) => Array.from(bytes, (byte) => String.fromCharCode(byte)).join("");
  const defaults = labelImageOptions({});
  const noDither = () => {
    throw new Error("not dithered");
  };

  it("writes a ZPL graphic field, ^GFA with the byte counts and upper-case hex", () => {
    const bytes = rasterToLabel(pixels(["#......#", "........"]), 8, 2, "zpl", defaults, noDither);
    expect(text(bytes)).toBe("^GFA,2,2,1,8100");
  });

  it("pads a width that is not a multiple of 8 with white, as QZ Tray does", () => {
    const bytes = rasterToLabel(pixels(["#########"]), 9, 1, "zpl", defaults, noDither);
    expect(text(bytes)).toBe("^GFA,2,2,2,FF80");
  });

  it("writes EPL's GW with the position, in binary, where 0 is black", () => {
    const options = labelImageOptions({ x: 10, y: 20 });
    const bytes = rasterToLabel(pixels(["#......."]), 8, 1, "epl", options, noDither);
    expect(Array.from(bytes)).toEqual([...Array.from("GW10,20,1,1,", (c) => c.charCodeAt(0)), 0x7f, 0x0a]);
  });

  it("writes CPCL's EG with the size, the position and hex", () => {
    const options = labelImageOptions({ x: "5", y: "6" });
    expect(text(rasterToLabel(pixels(["#......."]), 8, 1, "cpcl", options, noDither))).toBe("EG 1 1 5 6 80\r\n");
  });

  it("writes SBPL's ESC G H with three-digit sizes, the rows counted in eights", () => {
    const rows = Array(8).fill("#.......");
    expect(text(rasterToLabel(pixels(rows), 8, 8, "sbpl", defaults, noDither))).toBe("\x1bGH001001" + "80".repeat(8));
  });

  it("decides black by luma under QZ's threshold, 127 by default, with no dithering", () => {
    const grey = new Uint8ClampedArray(8 * 4).fill(150);
    for (let alpha = 3; alpha < grey.length; alpha += 4) grey[alpha] = 255;
    expect(text(rasterToLabel(grey, 8, 1, "zpl", defaults, noDither))).toBe("^GFA,1,1,1,00");
    expect(text(rasterToLabel(grey, 8, 1, "zpl", labelImageOptions({ threshold: 200 }), noDither))).toBe("^GFA,1,1,1,FF");
  });

  it("dithers only when the page asks for it", () => {
    const dither = vi.fn(() => new Uint8Array(8).fill(1));
    const bytes = rasterToLabel(pixels(["........"]), 8, 1, "zpl", labelImageOptions({ quantization: "dither" }), dither);
    expect(dither).toHaveBeenCalledOnce();
    expect(text(bytes)).toBe("^GFA,1,1,1,FF");
  });
});

describe("raw jobs with the file flavor", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const fileJob = (url: string) => ({ type: "raw", format: "command", flavor: "file", data: url });

  it("downloads the file in the page and sends its bytes unchanged, as QZ Tray does", async () => {
    const receipt = Uint8Array.from([0x1b, 0x40, 0x48, 0x69, 0x0a, 0x1d, 0x56, 0x00]);
    const download = vi.fn(async () => new Response(receipt));
    vi.stubGlobal("fetch", download);

    const rendered = await renderElements([fileJob("https://shop.test/receipt.bin")], {});

    expect(download).toHaveBeenCalledWith("https://shop.test/receipt.bin");
    expect(Array.from(jobToBytes({ printer: "kitchen", options: {}, data: rendered }))).toEqual(Array.from(receipt));
  });

  it("names CORS when the browser blocks a file on another site", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => Promise.reject(new TypeError("Failed to fetch"))));
    await expect(renderElements([fileJob("https://other.test/receipt.bin")], {})).rejects.toMatchObject({
      code: "PRINT_FAILED",
      message: expect.stringContaining("CORS"),
    });
  });

  it("reports the HTTP status when the file cannot be downloaded", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(null, { status: 404 })));
    await expect(renderElements([fileJob("https://shop.test/missing.bin")], {})).rejects.toMatchObject({
      code: "PRINT_FAILED",
      message: expect.stringContaining("404"),
    });
  });
});

describe("raw jobs in a label printer language", () => {
  it("sends raw ZPL commands to the printer unchanged, as QZ Tray does", async () => {
    // Label apps build their own ZPL (or TSPL, EPL) and send it as raw text;
    // only images, HTML and PDF need ESCPost to draw in a printer language.
    const zpl = "^XA^FO50,50^ADN,36,20^FDKitchen 12^FS^XZ";
    const job = { type: "raw", format: "command", flavor: "plain", data: zpl, options: { language: "ZPL" } };
    const rendered = await renderElements([job], {});
    const bytes = jobToBytes({ printer: "labels", options: {}, data: rendered });
    expect(new TextDecoder().decode(bytes)).toBe(zpl);
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
