import { describe, expect, it } from "vitest";
import { recordUnmatched, resolvePrinterName } from "../src/aliases";
import type { DaemonPrinter } from "../src/daemon";

const printers: DaemonPrinter[] = [
  { id: "tm-t20", name: "TM-T20", transport: "usb", profile: "NT-5890K", status: "ready" },
  { id: "kitchen", name: "Kitchen", transport: "network", profile: null, status: "ready" },
];

describe("resolvePrinterName", () => {
  it("matches an escpost entry name exactly", () => {
    expect(resolvePrinterName("TM-T20", printers, {})).toBe("tm-t20");
  });

  it("matches case-insensitively, because operators type what they remember", () => {
    expect(resolvePrinterName("tm-t20", printers, {})).toBe("tm-t20");
    expect(resolvePrinterName("KITCHEN", printers, {})).toBe("kitchen");
  });

  it("matches the id as well as the name", () => {
    expect(resolvePrinterName("kitchen", printers, {})).toBe("kitchen");
  });

  it("follows an alias the user created", () => {
    expect(resolvePrinterName("EPSON TM-T20II", printers, { "epson tm-t20ii": "tm-t20" })).toBe("tm-t20");
  });

  it("returns null for a name it has never heard of", () => {
    expect(resolvePrinterName("Star TSP100", printers, {})).toBeNull();
  });

  it("never resolves an alias that points at a printer which no longer exists", () => {
    expect(resolvePrinterName("Old Name", printers, { "old name": "removed-printer" })).toBeNull();
  });
});

describe("recordUnmatched", () => {
  it("keeps the name the page asked for, verbatim, with a timestamp", () => {
    const seen = recordUnmatched("Star TSP100", "https://pos.example.com", [], 1_000);
    expect(seen).toEqual([{ requested: "Star TSP100", origin: "https://pos.example.com", at: 1_000 }]);
  });

  it("moves a repeat request to the front rather than duplicating it", () => {
    const first = recordUnmatched("Star TSP100", "https://pos.example.com", [], 1_000);
    const second = recordUnmatched("Other", "https://pos.example.com", first, 2_000);
    const third = recordUnmatched("Star TSP100", "https://pos.example.com", second, 3_000);
    expect(third).toHaveLength(2);
    expect(third[0]).toEqual({ requested: "Star TSP100", origin: "https://pos.example.com", at: 3_000 });
  });

  it("keeps the list bounded so a looping integration cannot grow it forever", () => {
    let seen: ReturnType<typeof recordUnmatched> = [];
    for (let index = 0; index < 50; index++) seen = recordUnmatched(`printer-${index}`, "https://x.test", seen, index);
    expect(seen.length).toBeLessThanOrEqual(20);
    expect(seen[0]!.requested).toBe("printer-49");
  });
});
