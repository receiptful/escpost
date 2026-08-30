import { describe, expect, it, vi } from "vitest";
import { handleMessage, type WorkerDeps } from "../src/background";
import { PROTOCOL_VERSION } from "../src/protocol";
import { RenderCache } from "../src/render-cache";
import { memoryStorage, SessionStore } from "../src/session";

function deps(): WorkerDeps {
  return {
    daemon: {
      printers: vi.fn().mockResolvedValue([]),
      defaultPrinter: vi.fn().mockResolvedValue(null),
      print: vi.fn().mockResolvedValue({ jobId: "job-1" }),
      info: vi.fn().mockResolvedValue({ version: "0.5.0", platform: "linux", capabilities: [] }),
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
  };
}

describe("protocol version", () => {
  it("accepts a message at the current version", async () => {
    const result = await handleMessage(
      { op: "printers.list", payload: undefined, protocol: PROTOCOL_VERSION },
      "https://shop.test",
      deps(),
    );
    expect(result).toMatchObject({ ok: true });
  });

  it("rejects a newer package with VERSION_MISMATCH naming which side is behind", async () => {
    const result = await handleMessage(
      { op: "printers.list", payload: undefined, protocol: PROTOCOL_VERSION + 1 },
      "https://shop.test",
      deps(),
    );
    expect(result).toMatchObject({ ok: false, error: { code: "VERSION_MISMATCH" } });
    expect((result as any).error.message).toMatch(/extension/i);
  });

  it("rejects an older package with VERSION_MISMATCH naming which side is behind", async () => {
    const result = await handleMessage(
      { op: "printers.list", payload: undefined, protocol: PROTOCOL_VERSION - 1 },
      "https://shop.test",
      deps(),
    );
    expect(result).toMatchObject({ ok: false, error: { code: "VERSION_MISMATCH" } });
    expect((result as any).error.message).toMatch(/@escpost\/browser/);
  });

  it("treats a missing version as the current one, so the qz shims still work", async () => {
    // The window.qz and WebSocket surfaces are injected by this same extension build,
    // so they are never out of step and do not stamp a version.
    const result = await handleMessage({ op: "printers.list", payload: undefined }, "https://shop.test", deps());
    expect(result).toMatchObject({ ok: true });
  });

  it("checks the version before the origin gate, so the real problem is the one reported", async () => {
    const d = deps();
    d.isOriginGranted = vi.fn().mockResolvedValue(false);
    const result = await handleMessage(
      { op: "printers.list", payload: undefined, protocol: PROTOCOL_VERSION + 1 },
      "https://shop.test",
      d,
    );
    expect(result).toMatchObject({ ok: false, error: { code: "VERSION_MISMATCH" } });
  });
});
