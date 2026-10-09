import { describe, expect, it } from "vitest";
import { ACCOUNT_KEY, toPublishedAccount } from "../src/published-account";
import { memoryStorage, SessionStore, type AccountState } from "../src/session";
import {
  ACCOUNT_KEY as READER_ACCOUNT_KEY,
  readAccountSnapshot,
  type AccountSnapshot,
} from "../src/ui/account-snapshot";
import { describePopup, type PopupInput } from "../src/popup/state";

/** A connected daemon, so the popup state turns only on the account. */
function popupInput(snapshot: AccountSnapshot | null): PopupInput {
  return {
    daemon: { running: true, printers: [{ name: "TM-T20", detail: "USB" }] },
    account: snapshot,
    online: true,
    siteCount: 1,
    pendingSite: null,
  };
}

const JAN_15 = Date.UTC(2027, 0, 15, 12, 0, 0);
const FEB_1 = Date.UTC(2027, 1, 1);
const DEC_20 = Date.UTC(2027, 11, 20);
const NEXT_JAN_1 = Date.UTC(2028, 0, 1);

function account(overrides: Partial<AccountState> = {}): AccountState {
  return {
    email: "shop@example.com",
    orgId: "org-1",
    projectId: "project-1",
    signupAllowanceRemaining: 200,
    monthlyUsed: 0,
    monthlyLimit: 20,
    hasPaidAccess: false,
    ...overrides,
  };
}

describe("toPublishedAccount", () => {
  it("publishes the address as verified — an unverified one never has a session", () => {
    const published = toPublishedAccount(account(), JAN_15, null);

    expect(published.email).toBe("shop@example.com");
    // A2: a token only exists after the magic link is clicked, so anything
    // with a session is verified by construction.
    expect(published.verified).toBe(true);
    expect(published.signedInAt).toBe(JAN_15);
  });

  it("publishes an unspent signup grant as the signup allowance", () => {
    const published = toPublishedAccount(account({ signupAllowanceRemaining: 153 }), JAN_15, null);

    expect(published.allowance).toEqual({
      kind: "signup",
      remaining: 153,
      total: 200,
      resetsAt: null, // the signup grant never resets
    });
  });

  it("switches to the monthly allowance once the grant is spent", () => {
    const published = toPublishedAccount(
      account({ signupAllowanceRemaining: 0, monthlyUsed: 6 }),
      JAN_15,
      null,
    );

    expect(published.allowance).toEqual({
      kind: "monthly",
      remaining: 14,
      total: 20,
      resetsAt: FEB_1,
    });
  });

  it("rolls the reset date into the next year in December", () => {
    const published = toPublishedAccount(account({ signupAllowanceRemaining: 0 }), DEC_20, null);

    expect(published.allowance.resetsAt).toBe(NEXT_JAN_1);
  });

  it("never publishes a negative remaining count", () => {
    // The server can record a render that crossed the limit under a race;
    // a negative here would render as nonsense.
    const published = toPublishedAccount(
      account({ signupAllowanceRemaining: 0, monthlyUsed: 25 }),
      JAN_15,
      null,
    );

    expect(published.allowance.remaining).toBe(0);
  });

  it("keeps a paid org out of the exhausted state", () => {
    // The UI plan's Allowance has no "paid" kind, and a paid org has no
    // countdown to show. Full-of-full is the mapping that keeps U3's upsell
    // hidden, which is the behaviour that actually matters here.
    const published = toPublishedAccount(
      account({ hasPaidAccess: true, signupAllowanceRemaining: 0, monthlyUsed: 4_000 }),
      JAN_15,
      null,
    );

    expect(published.allowance.remaining).toBe(published.allowance.total);
    expect(published.allowance.remaining).toBeGreaterThan(0);
  });

  it("names a paid plan as paid rather than disguising it as a free monthly one", () => {
    const published = toPublishedAccount(
      account({ hasPaidAccess: true, signupAllowanceRemaining: 0, monthlyUsed: 4_000 }),
      JAN_15,
      null,
    );

    expect(published.allowance.kind).toBe("paid");
  });

  it("preserves the original sign-in time across an allowance update", () => {
    const published = toPublishedAccount(account({ signupAllowanceRemaining: 199 }), JAN_15, FEB_1);

    // "Signed in since" must not creep forward every time a receipt prints.
    expect(published.signedInAt).toBe(FEB_1);
  });
});

describe("SessionStore publishes alongside the private session", () => {
  it("publishes on sign-in without ever publishing the token", async () => {
    const storage = memoryStorage();
    const store = new SessionStore(storage, () => JAN_15);

    await store.write({ token: "rfx_secret", account: account() });

    const published = (await storage.get(ACCOUNT_KEY))[ACCOUNT_KEY] as Record<string, unknown>;
    expect(published.email).toBe("shop@example.com");
    expect(JSON.stringify(published)).not.toContain("rfx_secret");
  });

  it("republishes when the allowance changes", async () => {
    const storage = memoryStorage();
    const store = new SessionStore(storage, () => JAN_15);
    await store.write({ token: "rfx_secret", account: account() });

    await store.updateAccount(account({ signupAllowanceRemaining: 199 }));

    const published = (await storage.get(ACCOUNT_KEY))[ACCOUNT_KEY] as { allowance: { remaining: number } };
    expect(published.allowance.remaining).toBe(199);
  });

  it("removes the published snapshot on sign-out", async () => {
    const storage = memoryStorage();
    const store = new SessionStore(storage, () => JAN_15);
    await store.write({ token: "rfx_secret", account: account() });

    await store.clear();

    // A4: the popup must read "signed out", not a stale email.
    expect((await storage.get(ACCOUNT_KEY))[ACCOUNT_KEY]).toBeUndefined();
  });
});

/**
 * Neither plan tests across this boundary: the account plan tests the producer
 * and the UI plan tests the reader, and each is green while disagreeing with
 * the other. These tests run the real producer into the real reader, which is
 * the only way the drift is visible.
 */
describe("the published key round-trips through the popup's own reader", () => {
  it("is readable at all — a shape mismatch reads as signed out, silently", async () => {
    const storage = memoryStorage();
    await new SessionStore(storage, () => JAN_15).write({ token: "rfx_1", account: account() });

    const snapshot = readAccountSnapshot((await storage.get(ACCOUNT_KEY))[ACCOUNT_KEY]);

    expect(snapshot).not.toBeNull();
    expect(snapshot?.email).toBe("shop@example.com");
    expect(snapshot?.verified).toBe(true);
  });

  it("uses the same storage key on both sides", () => {
    expect(ACCOUNT_KEY).toBe(READER_ACCOUNT_KEY);
  });

  it("lands a fresh signup grant on the signed-in state, not on 'checking'", async () => {
    const storage = memoryStorage();
    await new SessionStore(storage, () => JAN_15).write({
      token: "rfx_1",
      account: account({ signupAllowanceRemaining: 153 }),
    });

    const snapshot = readAccountSnapshot((await storage.get(ACCOUNT_KEY))[ACCOUNT_KEY]);

    // known:false would render "Checking…" forever instead of the allowance.
    expect(snapshot?.allowance).toEqual({
      known: true,
      kind: "signup",
      remaining: 153,
      total: 200,
      resetsAt: null,
    });
    expect(describePopup(popupInput(snapshot)).kind).toBe("signed-in");
  });

  it("lands a spent monthly allowance on the exhausted state", async () => {
    const storage = memoryStorage();
    await new SessionStore(storage, () => JAN_15).write({
      token: "rfx_1",
      account: account({ signupAllowanceRemaining: 0, monthlyUsed: 20 }),
    });

    const snapshot = readAccountSnapshot((await storage.get(ACCOUNT_KEY))[ACCOUNT_KEY]);

    expect(describePopup(popupInput(snapshot)).kind).toBe("exhausted");
  });

  it("keeps a paid org off the upsell, which is the behaviour that matters", async () => {
    const storage = memoryStorage();
    await new SessionStore(storage, () => JAN_15).write({
      token: "rfx_1",
      account: account({ hasPaidAccess: true, signupAllowanceRemaining: 0, monthlyUsed: 4_000 }),
    });

    const snapshot = readAccountSnapshot((await storage.get(ACCOUNT_KEY))[ACCOUNT_KEY]);
    const view = describePopup(popupInput(snapshot));

    expect(view.kind).not.toBe("exhausted");
    expect(view.upsell).toBeNull();
  });

  it("shows a paid org its plan, not a free-tier countdown", async () => {
    const storage = memoryStorage();
    await new SessionStore(storage, () => JAN_15).write({
      token: "rfx_1",
      account: account({ hasPaidAccess: true, signupAllowanceRemaining: 0, monthlyUsed: 4_000 }),
    });

    const snapshot = readAccountSnapshot((await storage.get(ACCOUNT_KEY))[ACCOUNT_KEY]);
    const view = describePopup(popupInput(snapshot));

    expect(view.sections[2]?.rows).toContainEqual({ key: "HTML receipts", value: "Included" });
    expect(view.upsell).toBeNull();
  });

  it("reads as signed out after sign-out rather than showing a stale address", async () => {
    const storage = memoryStorage();
    const store = new SessionStore(storage, () => JAN_15);
    await store.write({ token: "rfx_1", account: account() });

    await store.clear();

    const snapshot = readAccountSnapshot((await storage.get(ACCOUNT_KEY))[ACCOUNT_KEY]);
    expect(snapshot).toBeNull();
    expect(describePopup(popupInput(snapshot)).kind).toBe("signed-out");
  });
});
