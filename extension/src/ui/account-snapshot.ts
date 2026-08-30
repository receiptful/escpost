/**
 * The contract between the UI and the account layer, which does not
 * exist yet. The account layer writes one `chrome.storage.local` key; everything the
 * popup and the settings page know about accounts comes through this reader.
 *
 * It is deliberately total. A popup that throws renders nothing, and a popup that
 * renders nothing is indistinguishable from a broken extension.
 */
export const ACCOUNT_KEY = "account";

export interface Allowance {
  /** False when the stored allowance could not be read. Not the same as "zero left". */
  known: boolean;
  /**
   * "paid" has no countdown: a paid org gets 1,000 receipts per active printer
   * with per-receipt overage beyond, so there is no limit to run out of. It is
   * a separate kind rather than a full monthly allowance because otherwise the
   * popup tells a paying customer they are on the free plan.
   */
  kind: "signup" | "monthly" | "paid";
  remaining: number;
  total: number;
  /** Epoch ms, or null for the signup grant, which never resets. */
  resetsAt: number | null;
}

export interface AccountSnapshot {
  email: string;
  /** An unverified address has no allowance, so this gates the signed-in states. */
  verified: boolean;
  signedInAt: number | null;
  allowance: Allowance;
}

const UNKNOWN_ALLOWANCE: Allowance = {
  known: false,
  kind: "monthly",
  remaining: 0,
  total: 0,
  resetsAt: null,
};

export function readAccountSnapshot(raw: unknown): AccountSnapshot | null {
  if (typeof raw !== "object" || raw === null) return null;
  const record = raw as Record<string, unknown>;

  const email = record["email"];
  if (typeof email !== "string" || email === "") return null;

  const signedInAt = record["signedInAt"];

  return {
    email,
    verified: record["verified"] === true,
    signedInAt: typeof signedInAt === "number" ? signedInAt : null,
    allowance: readAllowance(record["allowance"]),
  };
}

function readKind(raw: unknown): Allowance["kind"] {
  if (raw === "signup") return "signup";
  if (raw === "paid") return "paid";
  // An unrecognised kind falls back to the free monthly allowance: understating
  // what someone has is recoverable, claiming a plan they lack is not.
  return "monthly";
}

function readAllowance(raw: unknown): Allowance {
  if (typeof raw !== "object" || raw === null) return UNKNOWN_ALLOWANCE;
  const record = raw as Record<string, unknown>;

  const total = record["total"];
  const remaining = record["remaining"];
  if (typeof total !== "number" || total <= 0 || typeof remaining !== "number") return UNKNOWN_ALLOWANCE;

  const resetsAt = record["resetsAt"];

  return {
    known: true,
    kind: readKind(record["kind"]),
    remaining: Math.max(0, Math.min(remaining, total)),
    total,
    resetsAt: typeof resetsAt === "number" ? resetsAt : null,
  };
}
