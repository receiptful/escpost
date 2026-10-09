import { describe, expect, it, vi } from "vitest";
import { handleMessage, type WorkerDeps } from "../src/background";
import type { DaemonPrinter } from "../src/daemon";
import { RenderCache } from "../src/render-cache";
import { memoryStorage, SessionStore } from "../src/session";

const printers: DaemonPrinter[] = [
  { id: "tm-t20", name: "TM-T20", transport: "usb", profile: null, status: "ready" },
];

function deps(overrides: Partial<WorkerDeps> = {}): WorkerDeps {
  return {
    daemon: {
      printers: vi.fn().mockResolvedValue(printers),
      defaultPrinter: vi.fn().mockResolvedValue(printers[0]),
      print: vi.fn().mockResolvedValue({ jobId: "job-1" }),
      info: vi.fn().mockResolvedValue({ version: "0.5.0", platform: "linux", capabilities: ["usb"] }),
    } as unknown as WorkerDeps["daemon"],
    receiptful: {
      startAuth: vi.fn(),
      pollAuth: vi.fn(),
      account: vi.fn(),
      signOut: vi.fn(),
      registerPrinters: vi.fn(),
      render: vi.fn(),
      reportResult: vi.fn(),
    } as unknown as WorkerDeps["receiptful"],
    session: new SessionStore(memoryStorage()),
    renderCache: new RenderCache(memoryStorage()),
    isOriginGranted: vi.fn().mockResolvedValue(true),
    readAliases: vi.fn().mockResolvedValue({}),
    recordUnmatchedName: vi.fn().mockResolvedValue(undefined),
    badge: vi.fn(),
    now: () => 1_000,
    ...overrides,
  };
}

describe("handleMessage", () => {
  it("lists printers for a granted origin", async () => {
    const result = await handleMessage({ op: "printers.list", payload: undefined }, "https://shop.test", deps());
    expect(result).toEqual({ ok: true, data: printers });
  });

  it("prints, resolving the requested name to an escpost id", async () => {
    const d = deps({ readAliases: vi.fn().mockResolvedValue({ "epson tm-t20ii": "tm-t20" }) });
    const result = await handleMessage(
      { op: "print", payload: { printer: "EPSON TM-T20II", data: "G0A=" } },
      "https://shop.test",
      d,
    );
    expect(result).toEqual({ ok: true, data: { jobId: "job-1" } });
    expect(d.daemon.print).toHaveBeenCalledWith("tm-t20", "G0A=");
  });

  it("rejects an ungranted origin without opening a prompt", async () => {
    const d = deps({ isOriginGranted: vi.fn().mockResolvedValue(false) });
    const result = await handleMessage({ op: "printers.list", payload: undefined }, "https://evil.test", d);
    expect(result).toMatchObject({ ok: false, error: { code: "ORIGIN_NOT_GRANTED" } });
    expect(d.daemon.printers).not.toHaveBeenCalled();
    expect(d.badge).toHaveBeenCalled();
  });

  it("records an unmatched printer name instead of failing blind", async () => {
    const d = deps();
    const result = await handleMessage(
      { op: "print", payload: { printer: "Star TSP100", data: "G0A=" } },
      "https://shop.test",
      d,
    );
    expect(result).toMatchObject({ ok: false, error: { code: "PRINTER_NOT_FOUND" } });
    expect(d.recordUnmatchedName).toHaveBeenCalledWith("Star TSP100", "https://shop.test", 1_000);
    expect(d.daemon.print).not.toHaveBeenCalled();
  });

  it("names the printers it does know in the PRINTER_NOT_FOUND message", async () => {
    const result = await handleMessage(
      { op: "print", payload: { printer: "Star TSP100", data: "G0A=" } },
      "https://shop.test",
      deps(),
    );
    expect((result as any).error.message).toContain("TM-T20");
  });

  it("turns a daemon failure into a typed error rather than a raw exception", async () => {
    const d = deps();
    (d.daemon.printers as any).mockRejectedValue(new Error("boom"));
    const result = await handleMessage({ op: "printers.list", payload: undefined }, "https://shop.test", d);
    expect(result).toMatchObject({ ok: false, error: { code: "PRINT_FAILED" } });
  });

  it("rejects an unknown op rather than ignoring it, so no caller hangs", async () => {
    const result = await handleMessage({ op: "nonsense", payload: undefined }, "https://shop.test", deps());
    expect(result).toMatchObject({ ok: false });
  });
});
