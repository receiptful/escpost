import type { DaemonPrinter } from "./daemon";

/** Alias keys are lowercased requested names; values are escpost printer ids. */
export type AliasMap = Record<string, string>;

export interface UnmatchedRequest {
  requested: string;
  origin: string;
  at: number;
}

const MAX_UNMATCHED = 20;

/**
 * resolve whatever the page asked for to an escpost printer id.
 * Returns null rather than guessing — a wrong printer is worse than no printer.
 */
export function resolvePrinterName(requested: string, printers: DaemonPrinter[], aliases: AliasMap): string | null {
  const needle = requested.trim().toLowerCase();

  const direct = printers.find((printer) => printer.id.toLowerCase() === needle || printer.name.toLowerCase() === needle);
  if (direct) return direct.id;

  const aliased = aliases[needle];
  if (aliased === undefined) return null;

  // An alias to a printer that has since been unplugged or renamed is not a match.
  return printers.some((printer) => printer.id === aliased) ? aliased : null;
}

/**
 * remember the name the page asked for, so settings can offer a one-click alias.
 * Most recent first; bounded, because a retrying integration would otherwise fill storage.
 */
export function recordUnmatched(
  requested: string,
  origin: string,
  seen: UnmatchedRequest[],
  at: number,
): UnmatchedRequest[] {
  const withoutDuplicate = seen.filter(
    (entry) => !(entry.requested.toLowerCase() === requested.toLowerCase() && entry.origin === origin),
  );
  return [{ requested, origin, at }, ...withoutDuplicate].slice(0, MAX_UNMATCHED);
}
