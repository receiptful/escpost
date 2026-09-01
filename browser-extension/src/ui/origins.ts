import { isDaemonOrigin } from "../registration";

export type SiteOrigin = { origin: string; pattern: string };

export function currentSiteOrigin(url: string | undefined): SiteOrigin | null {
  if (url === undefined) return null;
  try {
    const parsed = new URL(url);
    if ((parsed.protocol !== "http:" && parsed.protocol !== "https:")
      || parsed.origin === "null"
      || parsed.hostname.includes("*")
      || isDaemonOrigin(parsed.origin)) {
      return null;
    }
    return { origin: parsed.origin, pattern: `${parsed.protocol}//${parsed.host}/*` };
  } catch {
    return null;
  }
}
