import { describe, expect, it, vi } from "vitest";
import { EscpostError } from "../../packages/browser/src/errors";
import { handleMessage, type WorkerDeps } from "../src/background";
import { RenderCache } from "../src/render-cache";
import { memoryStorage, SessionStore, type AccountState } from "../src/session";

const PAGE = "https://shop.test";

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

const RENDERED = {
  jobId: 7,
  data: "G0FSRU5ERVJFRA==",
  bucket: "signup_allowance",
  signupAllowanceRemaining: 199,
  monthlyUsed: 1,
};

/** Any property access at all throws. M1 has no exceptions, so neither does this. */
function forbiddenNetwork(): WorkerDeps["receiptful"] {
  return new Proxy(
    {},
    {
      get(_target, property) {
        throw new Error(`Raw printing touched Receiptful: ${String(property)}`);
      },
    },
  ) as WorkerDeps["receiptful"];
}

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
      startAuth: vi.fn(),
      pollAuth: vi.fn(),
      account: vi.fn(),
      signOut: vi.fn(),
      registerPrinters: vi.fn().mockResolvedValue(undefined),
      render: vi.fn().mockResolvedValue(RENDERED),
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

describe("raw printing is untouched by the account (M1, T8)", () => {
  it("makes no outbound request when signed out", async () => {
    const d = deps({ receiptful: forbiddenNetwork() });

    const result = await handleMessage({ op: "print", payload: { printer: "counter", data: "G0A=" } }, PAGE, d);

    expect(result).toEqual({ ok: true, data: { jobId: "job-1" } });
  });

  it("makes no outbound request when signed in either", async () => {
    // The tempting bug: "we have a token, so report usage". M1 says never.
    const d = deps({ receiptful: forbiddenNetwork() });
    await d.session.write({ token: "rfx_1", account });

    const result = await handleMessage({ op: "print", payload: { printer: "counter", data: "G0A=" } }, PAGE, d);

    expect(result).toEqual({ ok: true, data: { jobId: "job-1" } });
  });
});

describe("html printing", () => {
  it("refuses without an account, and renders nothing", async () => {
    const d = deps();
    const result = await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    expect(result).toMatchObject({ ok: false, error: { code: "NOT_SIGNED_IN" } });
    expect(d.receiptful.render).not.toHaveBeenCalled();
    expect(d.daemon.print).not.toHaveBeenCalled();
  });

  it("renders remotely and prints the bytes it gets back", async () => {
    const d = deps();
    await d.session.write({ token: "rfx_1", account });

    const result = await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    expect(result).toEqual({ ok: true, data: { jobId: "job-1" } });
    // R3: the daemon receives bytes and never learns HTML exists.
    expect(d.daemon.print).toHaveBeenCalledWith("counter", RENDERED.data);
  });

  it("asks for the render by device fingerprint, not by printer name", async () => {
    const d = deps();
    await d.session.write({ token: "rfx_1", account });

    await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    expect(d.receiptful.render).toHaveBeenCalledWith("rfx_1", {
      html: "<h1>x</h1>",
      profile: "NT-5890K",
      printerFingerprint: "usb:04b8:0202:S1",
    });
  });

  it("keeps the allowance shown in the popup up to date", async () => {
    const d = deps();
    await d.session.write({ token: "rfx_1", account });

    await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    expect((await d.session.read())?.account.signupAllowanceRemaining).toBe(199);
  });

  it("serves an identical reprint from cache with no second render", async () => {
    // R4 and M2: nothing was rendered, so nothing is charged.
    const d = deps();
    await d.session.write({ token: "rfx_1", account });
    await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    expect(d.receiptful.render).toHaveBeenCalledTimes(1);
    expect(d.daemon.print).toHaveBeenCalledTimes(2);
  });

  it("does not serve one site's cached render to another site (security finding 5)", async () => {
    const d = deps();
    await d.session.write({ token: "rfx_1", account });
    await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    // Same HTML, same printer, different site: it must render again rather than
    // answer instantly from a cache entry it can then infer the existence of.
    await handleMessage(
      { op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } },
      "https://other.test",
      d,
    );

    expect(d.receiptful.render).toHaveBeenCalledTimes(2);
  });

  it("says raw is unaffected when an uncached receipt cannot be rendered offline", async () => {
    const d = deps();
    await d.session.write({ token: "rfx_1", account });
    (d.receiptful.render as ReturnType<typeof vi.fn>).mockRejectedValue(
      new EscpostError("RENDER_UNAVAILABLE", "Receiptful could not be reached."),
    );

    const result = await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    expect(result).toMatchObject({ ok: false, error: { code: "RENDER_UNAVAILABLE" } });
    expect((result as { error: { message: string } }).error.message).toContain("Raw printing is unaffected.");
  });

  it("reports a local print failure so a spent unit is visible, not silent", async () => {
    // M5: the render succeeded and was charged; the paper jam happened after.
    const d = deps();
    await d.session.write({ token: "rfx_1", account });
    (d.daemon.print as ReturnType<typeof vi.fn>).mockRejectedValue(
      new EscpostError("PRINT_FAILED", "printer offline"),
    );

    const result = await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    expect(result).toMatchObject({ ok: false, error: { code: "PRINT_FAILED" } });
    expect(d.receiptful.reportResult).toHaveBeenCalledWith("rfx_1", 7, "failed", "printer offline");
  });

  it("reports a successful print too", async () => {
    const d = deps();
    await d.session.write({ token: "rfx_1", account });

    await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    expect(d.receiptful.reportResult).toHaveBeenCalledWith("rfx_1", 7, "completed");
  });
});

describe("the profile sent to /render", () => {
  it("uses the CANONICAL profile the server resolved, not the daemon's string", async () => {
    // The bug: the daemon reports an ordinary printers.toml value like
    // "tm-t88", which is not in Receiptful's catalog, so /render answered 422
    // and every HTML print failed while raw printing kept working.
    // TM-T88II is used here precisely because it is NOT the default, so this
    // proves the stored canonical name was used rather than the fallback.
    const storage = memoryStorage({ resolvedProfiles: { "entry:counter": "TM-T88II" } });
    const d = deps({ storage });
    (d.daemon.printers as any).mockResolvedValue([
      { id: "counter", name: "counter", transport: "usb", profile: "tm-t88", status: "ready" },
    ]);
    await d.session.write({ token: "rfx_1", account });

    await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    expect(d.receiptful.render).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({ profile: "TM-T88II" }),
    );
  });

  it("falls back to the default rather than sending a name it knows is rejected", async () => {
    // Nothing resolved yet (the daemon was down at sign-in, so registration
    // was skipped). Sending "tm-t88" would be sending a profile we know
    // /render refuses; the documented default is the honest choice.
    const storage = memoryStorage();
    const d = deps({ storage });
    (d.daemon.printers as any).mockResolvedValue([
      { id: "counter", name: "counter", transport: "usb", profile: "tm-t88", status: "ready" },
    ]);
    await d.session.write({ token: "rfx_1", account });

    await handleMessage({ op: "print", payload: { printer: "counter", html: "<h1>x</h1>" } }, PAGE, d);

    const sent = (d.receiptful.render as any).mock.calls[0][1];
    expect(sent.profile).toBe("NT-5890K");
    expect(sent.profile).not.toBe("tm-t88");
  });
});
