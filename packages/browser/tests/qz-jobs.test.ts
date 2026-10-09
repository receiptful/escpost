import { describe, expect, it } from "vitest";
import { bytesToBase64, jobToBytes, printerNameFrom, QZ_VERSION } from "../src/qz/jobs";

/** qz-tray.js:450-475 always sends every default option, never just the overridden ones. */
const ALL_DEFAULT_OPTIONS = {
  bounds: null, colorType: "color", copies: 1, density: 0, duplex: false,
  fallbackDensity: null, interpolation: "bicubic", jobName: null, legacy: false,
  margins: 0, orientation: null, paperThickness: null, printerTray: null,
  rasterize: false, rotation: 0, scaleContent: true, size: null, units: "in",
  forceRaw: false, encoding: null, spool: null,
};

describe("jobToBytes", () => {
  it("reports the version qz-tray.js applies no legacy rewrites for", () => {
    expect(QZ_VERSION).toBe("2.2.4");
  });

  it("treats a bare string element as raw/command/plain, which the library never expands", () => {
    const bytes = jobToBytes({ printer: { name: "TM-T20" }, options: ALL_DEFAULT_OPTIONS, data: ["\x1b@hello"] });
    expect(bytesToBase64(bytes)).toBe("G0BoZWxsbw==");
  });

  it("concatenates every element into one job", () => {
    const bytes = jobToBytes({ data: ["\x1b@", "hello"] });
    expect(bytesToBase64(bytes)).toBe("G0BoZWxsbw==");
  });

  it("decodes the base64 flavor", () => {
    const bytes = jobToBytes({ data: [{ type: "raw", format: "command", flavor: "base64", data: "G0A=" }] });
    expect(Array.from(bytes)).toEqual([0x1b, 0x40]);
  });

  it("decodes the hex flavor through the separators people actually paste", () => {
    const bytes = jobToBytes({ data: [{ flavor: "hex", data: "1b:40 68-65 0x6c 6c 6f" }] });
    expect(bytesToBase64(bytes)).toBe("G0BoZWxsbw==");
  });

  it("rejects a hex string that is not whole bytes rather than dropping a nibble", () => {
    expect(() => jobToBytes({ data: [{ flavor: "hex", data: "1b4" }] })).toThrow(/whole bytes/);
  });

  it("honours copies by repeating the payload", () => {
    const bytes = jobToBytes({ options: { ...ALL_DEFAULT_OPTIONS, copies: 3 }, data: ["HI"] });
    expect(bytesToBase64(bytes)).toBe(bytesToBase64(new TextEncoder().encode("HIHIHI")));
  });

  it("ignores a nonsense copies value instead of printing zero or a million receipts", () => {
    expect(jobToBytes({ options: { copies: 0 }, data: ["HI"] })).toHaveLength(2);
    expect(jobToBytes({ options: { copies: "many" }, data: ["HI"] })).toHaveLength(2);
  });

  it("rejects a pixel job and names the supported path", () => {
    expect(() => jobToBytes({ data: [{ type: "pixel", format: "html", flavor: "plain", data: "<h1>x</h1>" }] }))
      .toThrow(/@escpost\/browser/);
    expect(() => jobToBytes({ data: [{ type: "PIXEL", format: "HTML", data: "<h1>x</h1>" }] }))
      .toThrow(/@escpost\/browser/);
  });

  it("rejects a raw job whose format still needs rendering", () => {
    expect(() => jobToBytes({ data: [{ type: "raw", format: "pdf", data: "x.pdf" }] })).toThrow(/@escpost\/browser/);
    expect(() => jobToBytes({ data: [{ type: "raw", format: "image", data: "x.png" }] })).toThrow(/@escpost\/browser/);
  });

  it("rejects the file and xml flavors cleanly rather than printing the path", () => {
    expect(() => jobToBytes({ data: [{ flavor: "file", data: "https://x.test/receipt.bin" }] })).toThrow(/base64/);
    expect(() => jobToBytes({ data: [{ flavor: "xml", data: "https://x.test/receipt.xml" }] })).toThrow(/base64/);
  });

  it("carries the UNSUPPORTED_FORMAT code on every rendering refusal", () => {
    expect(() => jobToBytes({ data: [{ type: "pixel", data: "x" }] })).toThrow(
      expect.objectContaining({ code: "UNSUPPORTED_FORMAT" }),
    );
  });

  it("refuses a character that was never a byte instead of guessing an encoding", () => {
    expect(() => jobToBytes({ data: ["price: €5"] })).toThrow(/code page/);
  });

  it("passes Latin-1 code page bytes through untouched", () => {
    const bytes = jobToBytes({ data: ["café"] });
    expect(Array.from(bytes)).toEqual([0x63, 0x61, 0x66, 0xe9]);
  });
});

describe("printerNameFrom", () => {
  it("unwraps the object qz.configs.create builds from a string", () => {
    expect(printerNameFrom({ name: "TM-T20" })).toBe("TM-T20");
  });

  it("accepts a bare string", () => {
    expect(printerNameFrom("TM-T20")).toBe("TM-T20");
  });

  it("refuses print-to-file and print-to-host, which escpost does not do", () => {
    expect(() => printerNameFrom({ file: "/tmp/out.bin" })).toThrow(/printer name/);
    expect(() => printerNameFrom({ host: "10.0.0.5", port: 9180 })).toThrow(/printer name/);
  });

  it("refuses a job that named no printer at all", () => {
    expect(() => printerNameFrom(undefined)).toThrow(expect.objectContaining({ code: "PRINTER_NOT_FOUND" }));
  });
});
