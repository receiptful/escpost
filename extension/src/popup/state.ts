import type { AccountSnapshot } from "../ui/account-snapshot";
import { formatDay } from "../ui/format";
import { pill, type StatusPill, type Tone } from "../ui/status";

export interface PopupPrinter {
  name: string;
  /** "USB · default", "Network · unavailable" — whatever the daemon can tell us. */
  detail: string;
}

export type DaemonState = { running: false; message: string } | { running: true; printers: PopupPrinter[] };

/**
 * The active tab, when it is a site that could print but has not been granted
 *. Absent once granted, and absent for anything that is not a grantable
 * web origin — the extension's own pages, chrome:// pages, the new-tab page.
 */
export interface PendingSite {
  /** What to show a person: "pos.thornbury.app". */
  origin: string;
  /** What chrome.permissions.request needs: "https://pos.thornbury.app/*". */
  pattern: string;
  /** They were asked and said no. The control stays; it just stops asserting. */
  denied: boolean;
  /** Allowed to print, but the open page loaded before the scripts existed.
   *  Reloading is all that is left, and the popup says so however the user
   *  arrives at it, not only in the session where they granted. */
  needsReload: boolean;
  /** The page is a QZ Tray integration. Known only once the popup is open,
   *  because an ungranted page cannot be inspected before then. */
  usesQz: boolean;
}

export interface PopupInput {
  daemon: DaemonState;
  account: AccountSnapshot | null;
  online: boolean;
  siteCount: number;
  pendingSite: PendingSite | null;
}

export type PopupKind = "no-daemon" | "signed-out" | "signed-in" | "exhausted" | "offline";

export type PopupAction =
  | "grant-site"
  | "reload-site"
  | "check-again"
  | "open-welcome"
  | "open-plans"
  | "open-settings"
  | "open-install-help";

export interface PopupRow {
  key: string;
  value?: string;
  pill?: StatusPill;
}

export interface PopupButton {
  label: string;
  action: PopupAction;
  style: "primary" | "ghost";
  /** Carried to the click handler as data-value. The grant needs the match
   *  pattern, and the handler cannot look it up without losing the gesture. */
  value?: string;
}

export interface StripPart {
  text: string;
  strong: boolean;
}

export interface FooterItem {
  label: string;
  action: PopupAction;
}

/**
 * The declaration order of these fields is also the order they render in
 * (`renderSection` in popup.ts walks them top to bottom). `lead` is a paragraph
 * that introduces a section; `note` is a footnote that qualifies it.
 */
export interface PopupSection {
  label?: string;
  strip?: { tone: "warn" | "out"; parts: StripPart[] };
  lead?: string;
  command?: string;
  rows?: PopupRow[];
  meter?: { fraction: number; tone: Tone };
  button?: PopupButton;
  note?: string;
  /** A message from the daemon we did not expect. Shown verbatim, in small type. */
  detail?: string;
}

export interface PopupView {
  kind: PopupKind;
  status: StatusPill;
  sections: PopupSection[];
  footer: FooterItem[];
  /** The paid upsell. Non-null in exactly one state, `exhausted`. */
  upsell: PopupButton | null;
}

const RAW_UNAFFECTED = "Raw ESC/POS printing is unaffected and still working normally.";
const INSTALL_COMMAND = "brew install escpost";
const NO_ACCOUNT_NEEDED = "Raw printing never needs an account.";

/**
 * the daemon answers "will my printer work right now", and it answers first.
 * The order of these branches is the whole of the precedence.
 */
export function describePopup(input: PopupInput): PopupView {
  // Truthiness, not `=== null`: describePopup is the entry point for the whole
  // popup, and a caller that omits the field would otherwise throw here and
  // render nothing at all -- which reads exactly like a broken extension.
  const grant = input.pendingSite ? grantSection(input.pendingSite) : null;

  if (!input.daemon.running) return noDaemon(input.daemon.message, grant);

  const printers = printerSection(input.daemon.printers);
  const footer = connectedFooter(input.siteCount);
  const account = input.account;

  // an unverified address has no allowance, so it is not a signed-in state.
  if (account === null || !account.verified) return signedOut(printers, grant, footer, account);
  if (!input.online) return offline(printers, grant, footer);
  // never sell a plan to someone who already has one, whatever the counts say.
  if (account.allowance.kind !== "paid" && account.allowance.known && account.allowance.remaining === 0) {
    return exhausted(account, printers, grant, footer);
  }
  return signedIn(account, printers, grant, footer);
}

/** Drops the grant section in when there is one, keeping printers first. */
function withGrant(sections: PopupSection[], grant: PopupSection | null): PopupSection[] {
  if (grant === null) return sections;
  return [sections[0] as PopupSection, grant, ...sections.slice(1)];
}

function noDaemon(message: string, grant: PopupSection | null): PopupView {
  return {
    kind: "no-daemon",
    status: pill("Not running", "warn"),
    sections: withGrant([
      {
        lead:
          "escpost isn’t running on this machine, so nothing can print yet. Start it and this updates on its own. " +
          "There is no certificate to install and no account to create.",
        command: INSTALL_COMMAND,
        button: { label: "Check again", action: "check-again", style: "ghost" },
        ...(isExpectedAbsence(message) ? {} : { detail: message }),
      },
    ], grant),
    footer: [
      { label: "Settings", action: "open-settings" },
      { label: "Install help", action: "open-install-help" },
    ],
    upsell: null,
  };
}

/** An absent daemon is the ordinary first-run state; any other message is news. */
function isExpectedAbsence(message: string): boolean {
  return message === "" || message.includes("daemon is not running");
}

/**
 * one click, once per site. A refusal stays silent: no prompt
 * ever opens mid-print — so this is where the offer lives, on the user's own
 * initiative, after they have opened the popup.
 */
function grantSection(site: PendingSite): PopupSection {
  // The scripts are registered per granted site, so the page that is already
  // open loaded without them. Saying so is the difference between "allowed,
  // nothing happened" and one obvious next step.
  if (site.needsReload) {
    return {
      label: "Site access",
      lead: `${site.origin} is allowed to print. Reload the page to finish.`,
      button: { label: "Reload the page", action: "reload-site", style: "primary", value: site.pattern },
      note: "Only this site is affected. You can revoke it in Settings.",
    };
  }

  return {
    label: "Site access",
    lead: site.denied
      ? `You declined access for ${site.origin}, so it still cannot print. You can allow it whenever you like.`
      : site.usesQz
        ? `${site.origin} is set up for QZ Tray. Allow it and escpost prints for it instead, with no certificate and no dialog.`
        : `${site.origin} cannot print until you allow it. One click, once, and never asked again.`,
    button: {
      label: "Allow this site to print",
      action: "grant-site",
      style: "primary",
      value: site.pattern,
    },
    note: "Only this site is affected. Nothing else changes, and you can revoke it in Settings.",
  };
}

function printerSection(printers: PopupPrinter[]): PopupSection {
  if (printers.length === 0) {
    return {
      label: "Printers",
      lead: "escpost is running, but has no printers configured yet.",
      command: "escpost printers discover",
    };
  }
  return {
    label: "Printers",
    rows: printers.map((printer) => ({ key: printer.name, value: printer.detail })),
  };
}

function signedOut(printers: PopupSection, grant: PopupSection | null, footer: FooterItem[], pending: AccountSnapshot | null): PopupView {
  const html: PopupSection =
    pending === null
      ? {
          rows: [
            { key: "Raw ESC/POS", value: "Unlimited" },
            { key: "HTML receipts", pill: pill("Locked", "mute") },
          ],
          button: { label: "Add an email to unlock HTML", action: "open-welcome", style: "primary" },
          note: NO_ACCOUNT_NEEDED,
        }
      : {
          rows: [
            { key: "Raw ESC/POS", value: "Unlimited" },
            { key: "HTML receipts", pill: pill("Pending", "mute") },
          ],
          button: { label: "Resend the link", action: "open-welcome", style: "ghost" },
          note:
            `We sent a link to ${pending.email}. Click it to unlock HTML receipts. ` +
            `${NO_ACCOUNT_NEEDED} It is working now.`,
        };

  return {
    kind: "signed-out",
    status: pill("Connected", "ok"),
    sections: withGrant([printers, html], grant),
    footer,
    upsell: null,
  };
}

function signedIn(account: AccountSnapshot, printers: PopupSection, grant: PopupSection | null, footer: FooterItem[]): PopupView {
  const allowance = account.allowance;

  const html: PopupSection = allowance.kind === "paid"
    ? {
        rows: [
          { key: "Raw ESC/POS", value: "Unlimited" },
          // No count and no meter: both read as "you are running out", and a
          // paid plan has nothing to run out of.
          { key: "HTML receipts", value: "Included" },
        ],
        note: "Your plan includes 1,000 receipts per active printer.",
      }
    : allowance.known
    ? {
        rows: [
          { key: "Raw ESC/POS", value: "Unlimited" },
          { key: "HTML receipts", value: `${allowance.remaining} left` },
        ],
        meter: {
          fraction: allowance.remaining / allowance.total,
          tone: meterTone(allowance.remaining, allowance.total),
        },
        note:
          allowance.kind === "signup"
            ? "Signup allowance. After that, 20 a month free."
            : `${allowance.total} a month free.` +
              (allowance.resetsAt === null ? "" : ` Resets ${formatDay(allowance.resetsAt)}.`),
      }
    : {
        rows: [
          { key: "Raw ESC/POS", value: "Unlimited" },
          { key: "HTML receipts", value: "Checking…" },
        ],
        note: "Your HTML allowance will appear here once it has been checked. Raw printing is unlimited either way.",
      };

  return {
    kind: "signed-in",
    status: pill("Connected", "ok"),
    sections: withGrant([printers, { rows: [{ key: account.email, value: "Verified" }] }, html], grant),
    footer,
    upsell: null,
  };
}

function offline(printers: PopupSection, grant: PopupSection | null, footer: FooterItem[]): PopupView {
  return {
    kind: "offline",
    status: pill("Offline", "warn"),
    sections: withGrant([
      printers,
      {
        strip: {
          tone: "warn",
          parts: [
            { text: "Can’t reach Receiptful, so new HTML receipts can’t be rendered. ", strong: false },
            { text: RAW_UNAFFECTED, strong: true },
          ],
        },
      },
      {
        rows: [
          { key: "Raw ESC/POS", value: "Working" },
          { key: "Reprint last receipt", value: "Available" },
        ],
        note: "Recently printed receipts are cached, so reprinting one still works while you’re offline.",
      },
    ], grant),
    footer,
    upsell: null,
  };
}

function exhausted(account: AccountSnapshot, printers: PopupSection, grant: PopupSection | null, footer: FooterItem[]): PopupView {
  const allowance = account.allowance;
  const used =
    allowance.kind === "signup"
      ? `You’ve used all ${allowance.total} signup receipts.`
      : `You’ve used this month’s ${allowance.total}.`;
  const resets = allowance.resetsAt === null ? "" : ` Resets ${formatDay(allowance.resetsAt)} otherwise.`;
  const upsell: PopupButton = { label: "Add a printer plan · $5/mo", action: "open-plans", style: "primary" };

  return {
    kind: "exhausted",
    // The printer still works, so the pill still says so. Only HTML has stopped.
    status: pill("Connected", "ok"),
    sections: withGrant([
      printers,
      {
        strip: {
          tone: "out",
          parts: [
            { text: "HTML printing is paused. ", strong: true },
            { text: `${used} ${RAW_UNAFFECTED}`, strong: false },
          ],
        },
        rows: [{ key: "HTML receipts", value: `0 of ${allowance.total} left` }],
        // Filled, not empty: the bar has to read as blocked rather than as "nothing used".
        meter: { fraction: 1, tone: "out" },
      },
      {
        button: upsell,
        note: `Includes 1,000 receipts per printer.${resets}`,
      },
    ], grant),
    footer,
    upsell,
  };
}

function meterTone(remaining: number, total: number): Tone {
  return remaining / total <= 0.2 ? "warn" : "ok";
}

function connectedFooter(siteCount: number): FooterItem[] {
  const sites = siteCount === 0 ? "No sites yet" : `${siteCount} site${siteCount === 1 ? "" : "s"}`;
  return [
    { label: sites, action: "open-settings" },
    { label: "Settings", action: "open-settings" },
  ];
}
