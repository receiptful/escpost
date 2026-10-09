import type { AccountState } from "./session";

/**
 * Must match `ACCOUNT_KEY` in the UI plan's
 * `extension/src/ui/account-snapshot.ts`. Duplicated rather than imported so
 * this module does not depend on a file from another plan that may land
 * after it — but if the two ever disagree, the popup silently shows a signed
 * out account, so keep them the same.
 */
export const ACCOUNT_KEY = "account";

export interface PublishedAllowance {
  kind: "signup" | "monthly" | "paid";
  remaining: number;
  total: number;
  /** Epoch ms, or null for the signup grant, which never resets. */
  resetsAt: number | null;
}

export interface PublishedAccount {
  email: string;
  verified: boolean;
  signedInAt: number;
  allowance: PublishedAllowance;
}

/** The signup grant's size, mirrored from shared/config.py so the popup can
 *  show "153 of 200" without a round trip. */
const SIGNUP_ALLOWANCE_TOTAL = 200;

/**
 * The public projection of a session: what the popup and settings page are
 * allowed to know.
 *
 * The token is deliberately absent. Two UI surfaces read this key and
 * neither has any business holding a credential.
 *
 * `signedInAt` is carried over from the previous snapshot when there is one,
 * so "signed in since" does not creep forward every time a receipt prints.
 */
export function toPublishedAccount(
  account: AccountState,
  now: number,
  previousSignedInAt: number | null,
): PublishedAccount {
  return {
    email: account.email,
    // a token only exists after a magic link was clicked, so anything
    // holding a session is verified by construction.
    verified: true,
    signedInAt: previousSignedInAt ?? now,
    allowance: toAllowance(account, now),
  };
}

function toAllowance(account: AccountState, now: number): PublishedAllowance {
  if (account.hasPaidAccess) {
    // A paid org has no countdown: 1,000 receipts per active printer with
    // per-receipt overage beyond. remaining is still published full-of-full so
    // that a reader which does not know "paid" cannot land on the exhausted
    // state and offer a plan to someone who already pays.
    return {
      kind: "paid",
      remaining: account.monthlyLimit,
      total: account.monthlyLimit,
      resetsAt: firstOfNextMonth(now),
    };
  }

  if (account.signupAllowanceRemaining > 0) {
    return {
      kind: "signup",
      remaining: account.signupAllowanceRemaining,
      total: SIGNUP_ALLOWANCE_TOTAL,
      resetsAt: null,
    };
  }

  return {
    kind: "monthly",
    remaining: Math.max(0, account.monthlyLimit - account.monthlyUsed),
    total: account.monthlyLimit,
    resetsAt: firstOfNextMonth(now),
  };
}

/** The server counts by calendar month in UTC (billing_service
 *  .get_monthly_receipt_count), so the reset shown has to agree with it. */
function firstOfNextMonth(now: number): number {
  const date = new Date(now);
  return Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + 1, 1);
}
