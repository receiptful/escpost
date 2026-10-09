import { ACCOUNT_KEY, toPublishedAccount, type PublishedAccount } from "./published-account";

/** The account state the popup renders; mirrors the server's /account shape. */
export interface AccountState {
  email: string;
  orgId: string;
  projectId: string;
  signupAllowanceRemaining: number;
  monthlyUsed: number;
  monthlyLimit: number;
  hasPaidAccess: boolean;
}

export interface Session {
  token: string;
  account: AccountState;
}

/**
 * The slice of chrome.storage this module needs. MV3's
 * chrome.storage.local already returns promises from all three, so the real
 * object satisfies this without a wrapper — and no test needs a `chrome`
 * global.
 */
export interface StorageArea {
  get(key: string): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
  remove(key: string): Promise<void>;
}

export const SESSION_KEY = "session";

export class SessionStore {
  readonly #storage: StorageArea;
  readonly #now: () => number;

  constructor(storage: StorageArea, now: () => number = () => Date.now()) {
    this.#storage = storage;
    this.#now = now;
  }

  async read(): Promise<Session | null> {
    const stored = (await this.#storage.get(SESSION_KEY))[SESSION_KEY];
    return isSession(stored) ? stored : null;
  }

  async write(session: Session): Promise<void> {
    await this.#storage.set({ [SESSION_KEY]: session });
    await this.#publish(session.account);
  }

  /** Refresh the account after a render or an /account call. Returns null —
   *  and writes nothing — if nobody is signed in, so a stale response cannot
   *  resurrect a session that sign-out just removed. */
  async updateAccount(account: AccountState): Promise<Session | null> {
    const current = await this.read();
    if (current === null) return null;
    const next: Session = { token: current.token, account };
    await this.write(next);
    return next;
  }

  async clear(): Promise<void> {
    await this.#storage.remove(SESSION_KEY);
    // the popup must read "signed out", not a stale email.
    await this.#storage.remove(ACCOUNT_KEY);
  }

  /** The public projection, written in lockstep with the private session so
   *  the two can never disagree. The UI plan's popup and settings page read
   *  this key and nothing else. */
  async #publish(account: AccountState): Promise<void> {
    const previous = (await this.#storage.get(ACCOUNT_KEY))[ACCOUNT_KEY] as
      | Partial<PublishedAccount>
      | undefined;
    const signedInAt = typeof previous?.signedInAt === "number" ? previous.signedInAt : null;
    await this.#storage.set({
      [ACCOUNT_KEY]: toPublishedAccount(account, this.#now(), signedInAt),
    });
  }
}

/** A crash between two writes must read as signed out, not as an exception
 *  thrown from every message the worker handles. */
function isSession(value: unknown): value is Session {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as { token?: unknown; account?: unknown };
  if (typeof candidate.token !== "string" || candidate.token.length === 0) return false;
  const account = candidate.account as AccountState | undefined;
  return (
    typeof account === "object" &&
    account !== null &&
    typeof account.email === "string" &&
    typeof account.signupAllowanceRemaining === "number" &&
    typeof account.monthlyUsed === "number" &&
    typeof account.monthlyLimit === "number" &&
    typeof account.hasPaidAccess === "boolean"
  );
}

/** An in-memory StorageArea, used by the session, welcome and popup tests. */
export function memoryStorage(seed: Record<string, unknown> = {}): StorageArea {
  const data = new Map(Object.entries(seed));
  return {
    async get(key) {
      return data.has(key) ? { [key]: data.get(key) } : {};
    },
    async set(items) {
      for (const [key, value] of Object.entries(items)) data.set(key, value);
    },
    async remove(key) {
      data.delete(key);
    },
  };
}
