import { describe, expect, it } from "vitest";
import { memoryStorage, SessionStore, type AccountState, type Session } from "../src/session";

const account: AccountState = {
  email: "shop@example.com",
  orgId: "org-1",
  projectId: "project-1",
  signupAllowanceRemaining: 200,
  monthlyUsed: 0,
  monthlyLimit: 20,
  hasPaidAccess: false,
};

const session: Session = { token: "rfx_abc", account };

describe("SessionStore", () => {
  it("reports no session before anyone signs in", async () => {
    expect(await new SessionStore(memoryStorage()).read()).toBeNull();
  });

  it("round-trips a session", async () => {
    const store = new SessionStore(memoryStorage());
    await store.write(session);
    expect(await store.read()).toEqual(session);
  });

  it("clears on sign-out", async () => {
    const store = new SessionStore(memoryStorage());
    await store.write(session);
    await store.clear();
    expect(await store.read()).toBeNull();
  });

  it("survives a recycled worker", async () => {
    // A3/E3: the worker is torn down and rebuilt around the same storage.
    const storage = memoryStorage();
    await new SessionStore(storage).write(session);
    expect(await new SessionStore(storage).read()).toEqual(session);
  });

  it("treats a half-written value as no session rather than throwing", async () => {
    const storage = memoryStorage({ session: { token: "rfx_abc" } });
    // A crash between two storage writes must present as signed out, not as
    // an exception thrown from every message the worker handles.
    expect(await new SessionStore(storage).read()).toBeNull();
  });

  it("refreshes the account without disturbing the token", async () => {
    const store = new SessionStore(memoryStorage());
    await store.write(session);

    const updated = await store.updateAccount({ ...account, signupAllowanceRemaining: 199 });

    expect(updated?.token).toBe("rfx_abc");
    expect(updated?.account.signupAllowanceRemaining).toBe(199);
    expect((await store.read())?.account.signupAllowanceRemaining).toBe(199);
  });

  it("does not invent a session when refreshing an empty store", async () => {
    const storage = memoryStorage();
    const store = new SessionStore(storage);

    expect(await store.updateAccount(account)).toBeNull();
    expect(await store.read()).toBeNull();
  });
});
