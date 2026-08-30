/**
 * Where the extension reaches Receiptful.
 *
 * Set `ESCPOST_API_BASE` at build time to point a build at a local API:
 *
 *     ESCPOST_API_BASE=http://localhost:8000 bun run build
 *
 * The value is substituted by Vite's `define` (see vite.config.ts), which also
 * rewrites manifest.json's host_permissions and the auth-bridge match pattern
 * so all three stay in agreement. Editing this file to test locally is exactly
 * the friction it exists to remove: the base appears in three places, and a
 * hand-edited constant silently fails the auth-bridge origin tests.
 *
 * Under vitest there is no `define`, so the identifier is absent and the
 * production base stands — tests assert the shipped origin, never a dev one.
 */
declare const __ESCPOST_API_BASE__: string | undefined;

export const PRODUCTION_API_BASE = "https://api.receiptful.io";

export const RECEIPTFUL_BASE =
  typeof __ESCPOST_API_BASE__ === "string" ? __ESCPOST_API_BASE__ : PRODUCTION_API_BASE;

export const DAEMON_HOST = "127.0.0.1";

/**
 * Where escpost might be listening.
 *
 * It binds the first free port from 9000 upward rather than insisting on one,
 * so a machine with something else on 9000 puts it on 9001. Ten covers any
 * realistic collision; sweeping the whole range would cost ninety requests to
 * cover a case that does not happen.
 */
export const DAEMON_PORTS = [9000, 9001, 9002, 9003, 9004, 9005, 9006, 9007, 9008, 9009];

/** Where to look first, and what to fall back to when nothing answers. */
export const DAEMON_BASE = `http://${DAEMON_HOST}:${DAEMON_PORTS[0]}`;
