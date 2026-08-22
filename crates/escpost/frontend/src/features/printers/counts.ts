/**
 * `1 printer` rather than `1 printers`. One implementation, because this
 * exact bug shipped once already — an empty state that read "All 1 discovered
 * printers are already configured" — and a second copy of the rule is how it
 * comes back.
 *
 * English regular plurals only, which is all this interface counts: printers,
 * networks, probes.
 */
export function countOf(count: number, noun: string) {
  return `${count.toLocaleString()} ${noun}${count === 1 ? "" : "s"}`;
}
