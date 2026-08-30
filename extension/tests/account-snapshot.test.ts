import { describe, expect, it } from "vitest";
import { readAccountSnapshot } from "../src/ui/account-snapshot";

const complete = {
  email: "sam@bluebirdcafe.co",
  verified: true,
  signedInAt: Date.UTC(2026, 7, 4),
  allowance: { kind: "signup", remaining: 153, total: 200, resetsAt: null },
};

describe("readAccountSnapshot", () => {
  it("reads a complete snapshot", () => {
    expect(readAccountSnapshot(complete)).toEqual({
      email: "sam@bluebirdcafe.co",
      verified: true,
      signedInAt: Date.UTC(2026, 7, 4),
      allowance: { known: true, kind: "signup", remaining: 153, total: 200, resetsAt: null },
    });
  });

  it("treats nothing stored at all as signed out", () => {
    expect(readAccountSnapshot(undefined)).toBeNull();
    expect(readAccountSnapshot(null)).toBeNull();
    expect(readAccountSnapshot("signed-in")).toBeNull();
  });

  it("treats a snapshot with no usable email as signed out", () => {
    expect(readAccountSnapshot({ verified: true })).toBeNull();
    expect(readAccountSnapshot({ email: "", verified: true })).toBeNull();
  });

  it("treats a missing verification flag as unverified, never as verified", () => {
    expect(readAccountSnapshot({ email: "sam@x.co" })?.verified).toBe(false);
    expect(readAccountSnapshot({ email: "sam@x.co", verified: "yes" })?.verified).toBe(false);
  });

  it("marks an unreadable allowance as unknown rather than as exhausted", () => {
    expect(readAccountSnapshot({ email: "sam@x.co", verified: true })?.allowance).toEqual({
      known: false,
      kind: "monthly",
      remaining: 0,
      total: 0,
      resetsAt: null,
    });
  });

  it("clamps a remaining count that exceeds its total", () => {
    const raw = { email: "sam@x.co", verified: true, allowance: { kind: "monthly", remaining: 99, total: 20 } };
    expect(readAccountSnapshot(raw)?.allowance).toMatchObject({ known: true, remaining: 20, total: 20 });
  });

  it("clamps a negative remaining count to zero", () => {
    const raw = { email: "sam@x.co", verified: true, allowance: { kind: "monthly", remaining: -3, total: 20 } };
    expect(readAccountSnapshot(raw)?.allowance).toMatchObject({ known: true, remaining: 0 });
  });

  it("reads a paid allowance, which has no countdown to show", () => {
    const raw = { email: "sam@x.co", verified: true, allowance: { kind: "paid", remaining: 20, total: 20 } };
    expect(readAccountSnapshot(raw)?.allowance).toMatchObject({ known: true, kind: "paid" });
  });

  it("falls back to the monthly allowance for an allowance kind it has never heard of", () => {
    const raw = { email: "sam@x.co", verified: true, allowance: { kind: "lifetime", remaining: 5, total: 20 } };
    expect(readAccountSnapshot(raw)?.allowance).toMatchObject({ kind: "monthly", remaining: 5 });
  });
});
