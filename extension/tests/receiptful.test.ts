import { describe, expect, it, vi } from "vitest";
import { EscpostError } from "../../packages/browser/src/errors";
import { ReceiptfulClient } from "../src/receiptful";

const ACCOUNT = {
  email: "shop@example.com",
  org_id: "org-1",
  project_id: "project-1",
  signup_allowance_remaining: 200,
  monthly_used: 0,
  monthly_limit: 20,
  has_paid_access: false,
};

function jsonFetch(status: number, body: unknown, capture?: (input: string, init: RequestInit) => void) {
  return vi.fn(async (input: string, init: RequestInit = {}) => {
    capture?.(input, init);
    return new Response(status === 204 ? null : JSON.stringify(body), {
      status,
      headers: { "Content-Type": "application/json" },
    });
  }) as unknown as typeof fetch;
}

describe("ReceiptfulClient", () => {
  it("starts a sign-in and gets back no credential at all", async () => {
    // The takeover was the poll handing a session token to whoever asked.
    // Returning nothing here is what closes it: the token goes to the browser
    // that opens the link, and nowhere else.
    let seen: RequestInit | undefined;
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(200, { expires_in_seconds: 900 }, (_i, init) => { seen = init; }),
    );

    const started = await client.startAuth("shop@example.com");

    expect(started).toEqual({ expiresInSeconds: 900 });
    expect(JSON.stringify(started)).not.toContain("rfp_");
    expect(JSON.stringify(started)).not.toContain("rfx_");
    expect(JSON.parse(String(seen?.body))).toEqual({ email: "shop@example.com" });
  });






  it("sends the bearer token when reading the account", async () => {
    const seen: { headers?: Record<string, string> } = {};
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(200, ACCOUNT, (_url, init) => {
        seen.headers = init.headers as Record<string, string>;
      }),
    );

    await client.account("rfx_1");

    expect(seen.headers?.Authorization).toBe("Bearer rfx_1");
  });

  it("signs out against a 204", async () => {
    const client = new ReceiptfulClient("https://api.test", jsonFetch(204, null));
    await expect(client.signOut("rfx_1")).resolves.toBeUndefined();
  });

  it("registers every printer in one request", async () => {
    const seen: { body?: string } = {};
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(200, { printers: [] }, (_url, init) => {
        seen.body = init.body as string;
      }),
    );

    await client.registerPrinters("rfx_1", [
      { fingerprint: "usb:04b8:0202:S1", strength: "strong", entry_id: "counter", name: "counter", profile: "NT-5890K" },
      { fingerprint: "entry:kitchen", strength: "weak", entry_id: "kitchen", name: "kitchen", profile: "NT-5890K" },
    ]);

    // One request, not one per printer: three tills signing in at once
    // should not be six round trips.
    expect(JSON.parse(seen.body ?? "{}").printers).toHaveLength(2);
  });

  it("returns rendered bytes and the allowance that is left", async () => {
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(200, {
        job_id: 7,
        data: "G0A=",
        bucket: "signup_allowance",
        signup_allowance_remaining: 199,
        monthly_used: 1,
      }),
    );

    const result = await client.render("rfx_1", {
      html: "<h1>x</h1>",
      profile: "NT-5890K",
      printerFingerprint: "usb:04b8:0202:S1",
    });

    expect(result).toEqual({
      jobId: 7,
      data: "G0A=",
      bucket: "signup_allowance",
      signupAllowanceRemaining: 199,
      monthlyUsed: 1,
    });
  });

  it("passes a typed server error straight through", async () => {
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(402, { detail: { code: "QUOTA_EXCEEDED", message: "All used up." } }),
    );

    await expect(
      client.render("rfx_1", { html: "<h1>x</h1>", profile: "NT-5890K", printerFingerprint: "f" }),
    ).rejects.toMatchObject({ code: "QUOTA_EXCEEDED" });
  });

  it("turns an unreachable server into RENDER_UNAVAILABLE naming raw as unaffected", async () => {
    const client = new ReceiptfulClient(
      "https://api.test",
      vi.fn(async () => {
        throw new TypeError("Failed to fetch");
      }),
    );

    // T4: offline, an uncached HTML print must say so and must say that raw
    // still works. EscpostError appends that sentence for this code.
    const error = await client
      .render("rfx_1", { html: "<h1>x</h1>", profile: "NT-5890K", printerFingerprint: "f" })
      .catch((caught: EscpostError) => caught);

    expect((error as EscpostError).code).toBe("RENDER_UNAVAILABLE");
    expect((error as EscpostError).message).toContain("Raw printing is unaffected.");
  });
});


describe("FastAPI validation errors (422)", () => {
  it("names the offending field and reason instead of a bare status", async () => {
    // FastAPI's 422 body is an ARRAY of issues, not the {code,message} envelope
    // this surface uses everywhere else. `typeof [] === "object"`, so the
    // object branch matched, found no .message, and fell through to
    // "Receiptful returned 422." — an error nobody could act on. This is the
    // exact body the live api returned when the daemon reported "tm-t88".
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(422, {
        detail: [
          {
            type: "value_error",
            loc: ["body", "profile"],
            msg: "Value error, Unknown printer profile: tm-t88",
            input: "tm-t88",
          },
        ],
      }),
    );

    await expect(
      client.render("rfx_t", { html: "<h1>x</h1>", profile: "tm-t88", printerFingerprint: "usb:1" }),
    ).rejects.toThrow(/profile/);
    await expect(
      client.render("rfx_t", { html: "<h1>x</h1>", profile: "tm-t88", printerFingerprint: "usb:1" }),
    ).rejects.toThrow(/tm-t88/);
  });

  it("does not fall back to the bare status line", async () => {
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(422, {
        detail: [{ type: "value_error", loc: ["body", "html"], msg: "Value error, too long", input: "x" }],
      }),
    );

    await expect(
      client.render("rfx_t", { html: "x", profile: "NT-5890K", printerFingerprint: "usb:1" }),
    ).rejects.not.toThrow(/^Receiptful returned 422\.$/);
  });

  it("reports several validation issues rather than only the first", async () => {
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(422, {
        detail: [
          { loc: ["body", "profile"], msg: "Value error, Unknown printer profile: zzz" },
          { loc: ["body", "printer_fingerprint"], msg: "Field required" },
        ],
      }),
    );

    const caught = await client
      .render("rfx_t", { html: "x", profile: "zzz", printerFingerprint: "" })
      .catch((e) => e as Error);
    expect(caught.message).toMatch(/profile/);
    expect(caught.message).toMatch(/printer_fingerprint/);
  });

  it("still honours the typed {code,message} envelope everywhere else", async () => {
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(402, { detail: { code: "QUOTA_EXCEEDED", message: "All used up." } }),
    );

    await expect(
      client.render("rfx_t", { html: "x", profile: "NT-5890K", printerFingerprint: "usb:1" }),
    ).rejects.toMatchObject({ code: "QUOTA_EXCEEDED" });
    // (EscpostError appends "Raw printing is unaffected." to this code.)
  });
});

describe("registerPrinters", () => {
  it("returns the canonical profiles the server resolved", async () => {
    const client = new ReceiptfulClient(
      "https://api.test",
      jsonFetch(200, {
        printers: [
          { id: 1, fingerprint: "usb:1", label: "counter", profile: "NT-5890K", profile_matched: false },
        ],
      }),
    );

    const registered = await client.registerPrinters("rfx_t", [
      { fingerprint: "usb:1", strength: "strong", entry_id: "counter", name: "counter", profile: "tm-t88" },
    ]);

    expect(registered).toEqual([
      { fingerprint: "usb:1", profile: "NT-5890K", profileMatched: false },
    ]);
  });
});
