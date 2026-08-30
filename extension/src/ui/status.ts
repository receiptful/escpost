/**
 * Four tones, and only four. Semantic colour is reserved for status: nothing
 * else in either surface is allowed to use these.
 *   ok    — working
 *   warn  — degraded, but something still works
 *   out   — a hard stop the user can act on (an exhausted allowance)
 *   mute  — not applicable yet (a feature that is locked, not broken)
 */
export type Tone = "ok" | "warn" | "out" | "mute";

export const TONES: readonly Tone[] = ["ok", "warn", "out", "mute"];

export interface StatusPill {
  label: string;
  tone: Tone;
  /** The form half of the signal. Real text, so it survives a colour-blind reader. */
  glyph: string;
}

const GLYPHS: Record<Tone, string> = {
  ok: "●",
  warn: "▲",
  out: "■",
  mute: "○",
};

export function pill(label: string, tone: Tone): StatusPill {
  return { label, tone, glyph: GLYPHS[tone] };
}
