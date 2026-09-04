import { DAEMON_HOST, DAEMON_PORTS } from "./config";

export function originPattern(origin: string | undefined): string | null {
  if (origin === undefined) return null;
  try {
    const url = new URL(origin);
    if ((url.protocol !== "http:" && url.protocol !== "https:") || url.origin === "null") return null;
    return `${url.protocol}//${url.host}/*`;
  } catch {
    return null;
  }
}

export function isDaemonOrigin(origin: string): boolean {
  try {
    const url = new URL(origin);
    return url.protocol === "http:" && url.hostname === DAEMON_HOST && DAEMON_PORTS.some((port) => port === Number(url.port));
  } catch {
    return false;
  }
}
