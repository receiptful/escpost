import type { AliasMap, UnmatchedRequest } from "../aliases";
import type { DaemonPrinter } from "../daemon";
import type { AccountSnapshot } from "../ui/account-snapshot";
import { formatDay } from "../ui/format";
import { displayOrigin } from "../ui/origins";

/** Optional decoration for the site list. Keyed by host permission pattern. */
export type GrantMeta = Record<string, { at?: number; via?: string }>;

/** Optional decoration for the alias list. Keyed by the lowercased requested name. */
export type AliasMeta = Record<string, { requested: string; origin: string | null; at: number }>;

export interface SettingsInput {
  originPatterns: string[];
  /** The manifest's own host_permissions: infrastructure, never a granted site. */
  declaredHosts: string[];
  grants: GrantMeta;
  aliases: AliasMap;
  aliasMeta: AliasMeta;
  unmatched: UnmatchedRequest[];
  printers: DaemonPrinter[];
  account: AccountSnapshot | null;
  daemonRunning: boolean;
  extensionVersion: string;
}

export interface SiteRow {
  origin: string;
  /** The host permission pattern. Revoking needs this, not the display form. */
  pattern: string;
  sub: string;
}

export interface AliasRow {
  requested: string;
  target: string;
  matched: boolean;
  sub: string;
}

export interface UnmatchedRow {
  requested: string;
  origin: string;
  sub: string;
}

export interface PrinterChoice {
  id: string;
  name: string;
}

export interface SettingsView {
  account: { email: string; sub: string } | null;
  sites: SiteRow[];
  aliases: AliasRow[];
  unmatched: UnmatchedRow[];
  printerChoices: PrinterChoice[];
  /** `title` rather than a fixed heading: a signup allowance has no monthly
   *  window, and heading it with one contradicts the row beneath it. */
  usage: { title: string; html: string; resets: string; raw: string } | null;
  about: { daemon: string; extension: string };
}

export function describeSettings(input: SettingsInput): SettingsView {
  return {
    account: describeAccount(input.account),
    sites: describeSites(input.originPatterns, input.grants, input.declaredHosts),
    aliases: describeAliases(input.aliases, input.aliasMeta, input.printers),
    unmatched: input.unmatched.map((entry) => describeUnmatched(entry)),
    printerChoices: input.printers.map((printer) => ({ id: printer.id, name: printer.name })),
    usage: describeUsage(input.account),
    about: {
      daemon: input.daemonRunning ? "running" : "not running",
      extension: input.extensionVersion,
    },
  };
}

function describeAccount(account: AccountSnapshot | null): SettingsView["account"] {
  if (account === null) return null;
  const state = account.verified ? "Verified" : "Awaiting the link";
  const when = account.signedInAt === null ? "" : ` · signed in ${formatDay(account.signedInAt)}`;
  return { email: account.email, sub: `${state}${when}` };
}

function describeSites(patterns: string[], grants: GrantMeta, declared: string[]): SiteRow[] {
  const rows: SiteRow[] = [];

  for (const pattern of patterns) {
    const origin = displayOrigin(pattern, declared);
    if (origin === null) continue;

    const meta = grants[pattern];
    const at = meta?.at;
    const via = meta?.via;
    const sub =
      at === undefined
        ? "granted when this site first asked to print"
        : `granted ${formatDay(at)}${via === undefined ? "" : ` · ${via}`}`;

    rows.push({ origin, pattern, sub });
  }

  return rows;
}

function describeAliases(aliases: AliasMap, meta: AliasMeta, printers: DaemonPrinter[]): AliasRow[] {
  return Object.keys(aliases)
    .sort()
    .map((key) => {
      const target = aliases[key] ?? "";
      const printer = printers.find((candidate) => candidate.id === target);
      const asked = meta[key];
      const origin = asked?.origin ?? null;

      return {
        // The alias map lowercases its keys, so the name the page actually used only
        // survives in the metadata. Falling back to the key is honest: that really is
        // all we stored for an alias created before this page existed.
        requested: asked?.requested ?? key,
        target: printer?.name ?? target,
        matched: printer !== undefined,
        sub: origin === null ? "created here" : `requested by ${hostOf(origin)}`,
      };
    });
}

function describeUnmatched(entry: UnmatchedRequest): UnmatchedRow {
  return {
    // verbatim, exactly as the page asked for it. This is the string an operator
    // has to recognise from someone else's source code.
    requested: entry.requested,
    origin: entry.origin,
    sub: `requested by ${hostOf(entry.origin)} on ${formatDay(entry.at)} · that print failed`,
  };
}

function describeUsage(account: AccountSnapshot | null): SettingsView["usage"] {
  if (account === null || !account.allowance.known) return null;
  const allowance = account.allowance;
  if (allowance.kind === "paid") {
    return {
      title: "Usage this month",
      html: "Included",
      resets: "1,000 per active printer",
      raw: "Unlimited",
    };
  }

  const monthly = allowance.resetsAt !== null;
  return {
    title: monthly ? "Usage this month" : "Usage",
    html: `${allowance.total - allowance.remaining} of ${allowance.total}`,
    resets: monthly ? `resets ${formatDay(allowance.resetsAt as number)}` : "the signup allowance, which does not reset",
    // Restated where someone might go looking for a limit that does not exist.
    raw: "Unlimited",
  };
}

function hostOf(origin: string): string {
  return origin.replace(/^https?:\/\//, "");
}
