import type { AccountSnapshot } from "../../src/ui/account-snapshot";
import type { PopupInput, PopupKind, PopupPrinter } from "../../src/popup/state";

export const PRINTERS: PopupPrinter[] = [
  { name: "TM-T20", detail: "USB · default" },
  { name: "Kitchen", detail: "Network" },
];

export const DAEMON_ABSENT =
  "The escpost daemon is not running. Start it with `escpost daemon`, or install escpost from escpost.dev.";

export function account(overrides: Partial<AccountSnapshot> = {}): AccountSnapshot {
  return {
    email: "sam@bluebirdcafe.co",
    verified: true,
    signedInAt: Date.UTC(2026, 7, 4),
    allowance: { known: true, kind: "signup", remaining: 153, total: 200, resetsAt: null },
    ...overrides,
  };
}

export const INPUTS: Record<PopupKind, PopupInput> = {
  "no-daemon": {
    daemon: { running: false, message: DAEMON_ABSENT },
    account: null,
    online: true,
    siteCount: 2,
    pendingSite: null,
  },
  "signed-out": {
    daemon: { running: true, printers: PRINTERS },
    account: null,
    online: true,
    siteCount: 2,
    pendingSite: null,
  },
  "signed-in": {
    daemon: { running: true, printers: PRINTERS },
    account: account(),
    online: true,
    siteCount: 2,
    pendingSite: null,
  },
  exhausted: {
    daemon: { running: true, printers: PRINTERS },
    account: account({
      allowance: { known: true, kind: "monthly", remaining: 0, total: 20, resetsAt: Date.UTC(2026, 8, 1) },
    }),
    online: true,
    siteCount: 2,
    pendingSite: null,
  },
  offline: {
    daemon: { running: true, printers: PRINTERS },
    account: account(),
    online: false,
    siteCount: 2,
    pendingSite: null,
  },
};

export const ALL_KINDS: readonly PopupKind[] = ["no-daemon", "signed-out", "signed-in", "exhausted", "offline"];

/** The four states in which the daemon is up, and the printer question has an answer. */
export const CONNECTED_KINDS: readonly PopupKind[] = ["signed-out", "signed-in", "exhausted", "offline"];
