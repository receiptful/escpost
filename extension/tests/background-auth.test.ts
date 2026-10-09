import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EscpostError } from "../../packages/browser/src/errors";
import { handleMessage, type WorkerDeps } from "../src/background";
import { RenderCache } from "../src/render-cache";
import { memoryStorage, SessionStore, type AccountState } from "../src/session";

const EXT_ID = "abcdefghijklmnop";
const EXT_ORIGIN = `chrome-extension://${EXT_ID}`;

// isExtensionOrigin compares against chrome.runtime.id specifically, so that
// another installed extension cannot drive this worker. That means these tests
// need the id the real runtime always has.
beforeEach(() => {
  (globalThis as unknown as { chrome: unknown }).chrome = { runtime: { id: EXT_ID } };
});

afterEach(() => {
  delete (globalThis as unknown as { chrome?: unknown }).chrome;
});

const account: AccountState = {
  email: "shop@example.com",
  orgId: "org-1",
  projectId: "project-1",
  signupAllowanceRemaining: 200,
  monthlyUsed: 0,
  monthlyLimit: 20,
  hasPaidAccess: false,
};

const printers = [
  {
    id: "counter",
    name: "counter",
    transport: "usb" as const,
    profile: "NT-5890K",
    status: "ready" as const,
    device: { usbVendorId: 0x04b8, usbProductId: 0x0202, usbSerial: "S1" },
  },
];

function deps(overrides: Partial<WorkerDeps> = {}): WorkerDeps {
  const storage = memoryStorage();
  return {
    daemon: {
      printers: vi.fn().mockResolvedValue(printers),
      defaultPrinter: vi.fn().mockResolvedValue(printers[0]),
      print: vi.fn().mockResolvedValue({ jobId: "job-1" }),
      info: vi.fn().mockResolvedValue({ version: "0.5.0", platform: "linux", capabilities: [] }),
    } as unknown as WorkerDeps["daemon"],
    receiptful: {
      startAuth: vi.fn().mockResolvedValue({ pollToken: "rfp_1", expiresInSeconds: 900 }),
      account: vi.fn().mockResolvedValue(account),
      signOut: vi.fn().mockResolvedValue(undefined),
      registerPrinters: vi.fn().mockResolvedValue(undefined),
      render: vi.fn(),
      reportResult: vi.fn().mockResolvedValue(undefined),
    } as unknown as WorkerDeps["receiptful"],
    storage,
    session: new SessionStore(storage),
    renderCache: new RenderCache(storage),
    isOriginGranted: vi.fn().mockResolvedValue(true),
    readAliases: vi.fn().mockResolvedValue({}),
    recordUnmatchedName: vi.fn().mockResolvedValue(undefined),
    badge: vi.fn(),
    now: () => 1_000,
    ...overrides,
  };
}

describe("account operations", () => {
  it("reports signed out before anyone signs in", async () => {
    const result = await handleMessage({ op: "auth.status", payload: undefined }, EXT_ORIGIN, deps());
    expect(result).toEqual({ ok: true, data: { signedIn: false, account: null } });
  });

  it("reports the account once signed in", async () => {
    const d = deps();
    await d.session.write({ token: "rfx_1", account });

    const result = await handleMessage({ op: "auth.status", payload: undefined }, EXT_ORIGIN, d);

    expect(result).toEqual({ ok: true, data: { signedIn: true, account } });
  });

  it("forwards the email to Receiptful and hands back only a poll token", async () => {
    const d = deps();
    const result = await handleMessage(
      { op: "auth.start", payload: { email: "shop@example.com" } },
      EXT_ORIGIN,
      d,
    );

    expect(d.receiptful.startAuth).toHaveBeenCalledWith("shop@example.com");
    expect(result).toEqual({ ok: true, data: { pollToken: "rfp_1", expiresInSeconds: 900 } });
  });





  it("signs out server-side and clears the token", async () => {
    const d = deps();
    await d.session.write({ token: "rfx_1", account });

    const result = await handleMessage({ op: "auth.signout", payload: undefined }, EXT_ORIGIN, d);

    expect(result).toEqual({ ok: true, data: { signedIn: false, account: null } });
    expect(d.receiptful.signOut).toHaveBeenCalledWith("rfx_1");
    expect(await d.session.read()).toBeNull();
  });

  it("refuses account operations from a web page", async () => {
    const d = deps();
    await d.session.write({ token: "rfx_1", account });

    for (const op of ["auth.status", "auth.start", "auth.signout"]) {
      const result = await handleMessage({ op, payload: { email: "x@y.z" } }, "https://shop.test", d);
      expect(result).toMatchObject({ ok: false, error: { code: "ORIGIN_NOT_GRANTED" } });
    }
    // A page that could do any of these could sign a merchant out mid-shift
    // or read their account back.
    expect(d.receiptful.signOut).not.toHaveBeenCalled();
    expect(d.receiptful.startAuth).not.toHaveBeenCalled();
    expect(await d.session.read()).not.toBeNull();
  });

  it("refuses account operations from another extension's pages", async () => {
    // Not covered by the plan, but the reason isExtensionOrigin pins the id:
    // a bare chrome-extension:// prefix check would let any installed
    // extension sign this merchant out.
    const d = deps();
    await d.session.write({ token: "rfx_1", account });

    const result = await handleMessage(
      { op: "auth.signout", payload: undefined },
      "chrome-extension://zzzzzzzzzzzzzzzz",
      d,
    );

    expect(result).toMatchObject({ ok: false, error: { code: "ORIGIN_NOT_GRANTED" } });
    expect(d.receiptful.signOut).not.toHaveBeenCalled();
  });
});

describe("profiles resolved at registration", () => {

  it("survives a server that does not report profiles yet", async () => {
    const storage = memoryStorage();
    const d = deps({ storage });
    (d.receiptful.account as any).mockResolvedValue(account);
    (d.receiptful.registerPrinters as any).mockResolvedValue(undefined);

    const result = await handleMessage(
      { op: "auth.bridge", payload: { token: "rfx_1" } },
      "https://api.receiptful.io",
      d,
    );

    expect(result).toMatchObject({ ok: true });
  });
});


describe("auth.bridge — the verify page hands over the token", () => {
  const API_ORIGIN = "https://api.receiptful.io";
  const TOKEN = "rfx_" + "b".repeat(64);

  it("signs in from a token delivered by the verify page", async () => {
    const storage = memoryStorage();
    const d = deps({ storage });
    (d.receiptful.account as any).mockResolvedValue(account);

    const result = await handleMessage(
      { op: "auth.bridge", payload: { token: TOKEN } },
      API_ORIGIN,
      d,
    );

    expect(result).toMatchObject({ ok: true });
    expect(d.receiptful.account).toHaveBeenCalledWith(TOKEN);
    expect((await d.session.read())?.token).toBe(TOKEN);
  });

  it("REFUSES a token offered by any other origin", async () => {
    // Without this, any granted page could sign the merchant into an
    // attacker's account by injecting a token of the attacker's choosing.
    const d = deps();

    const result = await handleMessage(
      { op: "auth.bridge", payload: { token: TOKEN } },
      "https://shop.test",
      d,
    );

    expect(result).toMatchObject({ ok: false });
    expect(d.receiptful.account).not.toHaveBeenCalled();
    expect(await d.session.read()).toBeNull();
  });

  it("refuses a token the server does not recognise", async () => {
    const d = deps();
    (d.receiptful.account as any).mockRejectedValue(
      new EscpostError("NOT_SIGNED_IN", "This sign-in is no longer valid."),
    );

    const result = await handleMessage(
      { op: "auth.bridge", payload: { token: "rfx_forged" } },
      API_ORIGIN,
      d,
    );

    expect(result).toMatchObject({ ok: false });
    expect(await d.session.read()).toBeNull();
  });

  it("rejects a malformed payload without touching the network", async () => {
    const d = deps();

    const result = await handleMessage({ op: "auth.bridge", payload: {} }, API_ORIGIN, d);

    expect(result).toMatchObject({ ok: false });
    expect(d.receiptful.account).not.toHaveBeenCalled();
  });

  it("registers the machine's printers on the way in", async () => {
    const d = deps();
    (d.receiptful.account as any).mockResolvedValue(account);

    await handleMessage({ op: "auth.bridge", payload: { token: TOKEN } }, API_ORIGIN, d);

    expect(d.receiptful.registerPrinters).toHaveBeenCalled();
  });
});
