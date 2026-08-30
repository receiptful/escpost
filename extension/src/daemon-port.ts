import { DAEMON_HOST, DAEMON_PORTS } from "./config";
import type { StorageArea } from "./session";

/**
 * Which port escpost is on.
 *
 * escpost takes the first free port in its range rather than insisting on one,
 * so a machine with something else on 9000 puts it on 9001 and a client that
 * assumes 9000 finds nothing. The symptom is silence, which is the worst thing
 * to diagnose on a shop floor.
 *
 * QZ Tray has the same arrangement and its client solves it the same way, by
 * trying each of eight ports in turn.
 */
const REMEMBERED_KEY = "daemonPort";

/** `/health` answers "ok" to anyone: it is not behind the origin rule, and it
 *  is the cheapest thing on the server. */
const PROBE_PATH = "/health";

/** Short. These are loopback connections that either answer at once or are not
 *  there, and the whole probe runs before the first print. */
const PROBE_TIMEOUT_MS = 400;

export interface PortStore {
  get(key: string): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
}

export function baseFor(port: number): string {
  return `http://${DAEMON_HOST}:${port}`;
}

/**
 * The base URL to talk to, or null when nothing answered.
 *
 * The remembered port is tried first and alone, because it is right almost
 * always and a hit costs one request. Only a miss pays for the sweep.
 */
export async function findDaemonBase(
  storage: PortStore,
  fetchImpl: typeof fetch = fetch,
): Promise<string | null> {
  const remembered = await rememberedPort(storage);
  if (remembered !== null && (await answers(remembered, fetchImpl))) return baseFor(remembered);

  for (const port of DAEMON_PORTS) {
    if (port === remembered) continue;
    if (!(await answers(port, fetchImpl))) continue;
    await storage.set({ [REMEMBERED_KEY]: port });
    return baseFor(port);
  }

  return null;
}

async function rememberedPort(storage: PortStore): Promise<number | null> {
  const stored = await storage.get(REMEMBERED_KEY);
  const port = stored[REMEMBERED_KEY];
  return typeof port === "number" && DAEMON_PORTS.includes(port) ? port : null;
}

async function answers(port: number, fetchImpl: typeof fetch): Promise<boolean> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  try {
    const response = await fetchImpl.bind(globalThis)(baseFor(port) + PROBE_PATH, {
      method: "GET",
      signal: controller.signal,
    });
    return response.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

/** Exported for the worker, which has `chrome.storage.local` rather than this shape. */
export function portStoreFrom(storage: StorageArea): PortStore {
  return {
    get: (keys) => storage.get(keys),
    set: (items) => storage.set(items),
  };
}
