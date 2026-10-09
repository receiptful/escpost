import type { AliasMap, UnmatchedRequest } from "../aliases";
import type { AliasMeta, GrantMeta } from "./model";

export interface AliasState {
  aliases: AliasMap;
  aliasMeta: AliasMeta;
  unmatched: UnmatchedRequest[];
}

function key(requested: string): string {
  // Must match resolvePrinterName's lookup in aliases.ts exactly, or an alias that
  // looks right in this page will never match a real print.
  return requested.trim().toLowerCase();
}

/** the one-click fix: map the name a page asked for onto a real escpost printer. */
export function createAlias(state: AliasState, requested: string, printerId: string, at: number): AliasState {
  const name = key(requested);
  const source = state.unmatched.find((entry) => key(entry.requested) === name);

  return {
    aliases: { ...state.aliases, [name]: printerId },
    aliasMeta: {
      ...state.aliasMeta,
      [name]: { requested: requested.trim(), origin: source?.origin ?? null, at },
    },
    // The alias map is global, so every site that asked for this name is now fixed.
    unmatched: state.unmatched.filter((entry) => key(entry.requested) !== name),
  };
}

export function removeAlias(state: AliasState, requested: string): AliasState {
  const name = key(requested);
  const aliases = { ...state.aliases };
  const aliasMeta = { ...state.aliasMeta };
  delete aliases[name];
  delete aliasMeta[name];
  return { ...state, aliases, aliasMeta };
}

export function dismissUnmatched(state: AliasState, requested: string, origin: string): AliasState {
  const name = key(requested);
  return {
    ...state,
    unmatched: state.unmatched.filter((entry) => !(key(entry.requested) === name && entry.origin === origin)),
  };
}

/** Revoking a site should not leave its grant record behind to reappear later. */
export function forgetGrant(grants: GrantMeta, pattern: string): GrantMeta {
  const next = { ...grants };
  delete next[pattern];
  return next;
}
