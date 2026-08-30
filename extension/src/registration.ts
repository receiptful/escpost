/**
 * Which pages the extension is present on.
 *
 * The scripts used to be declared in the manifest against `<all_urls>`, so
 * every page on the web got a patched `WebSocket` and a `qz` global, and Chrome
 * asked at install for permission to read and change data on every site. The
 * extension already had a per-site grant; the injection simply ignored it.
 *
 * They are registered at runtime instead, for granted origins only. A site the
 * user has not allowed gets nothing: no script, no globals, no listener.
 */

/** The slice of `chrome.scripting` this module needs, so tests need no `chrome`. */
export interface ScriptingArea {
  getRegisteredContentScripts(filter?: { ids?: string[] }): Promise<Array<{ id: string }>>;
  registerContentScripts(scripts: RegisteredScript[]): Promise<void>;
  unregisterContentScripts(filter?: { ids?: string[] }): Promise<void>;
}

export interface RegisteredScript {
  id: string;
  js: string[];
  matches: string[];
  runAt: "document_start";
  world: "ISOLATED" | "MAIN";
  allFrames: boolean;
  persistAcrossSessions: boolean;
}

/** The relay carries messages and holds the only `chrome.runtime` handle. */
export const RELAY_ID = "escpost-relay";

/** The QZ compatible surface, in the page's own context because that is where
 *  a page's `qz` and `WebSocket` live. */
export const COMPAT_ID = "escpost-qz-compat";

/**
 * `document_start` in both cases, and not negotiable for the compat pair: a
 * page that ships qz-tray.js captures `WebSocket` as it loads, so a patch that
 * arrives afterwards patches nothing.
 */
export function scriptsFor(origins: string[]): RegisteredScript[] {
  if (origins.length === 0) return [];
  return [
    {
      id: RELAY_ID,
      js: ["relay.js"],
      matches: origins,
      runAt: "document_start",
      world: "ISOLATED",
      allFrames: false,
      persistAcrossSessions: true,
    },
    {
      id: COMPAT_ID,
      js: ["ws-patch.js", "qz-shim.js"],
      matches: origins,
      runAt: "document_start",
      world: "MAIN",
      allFrames: false,
      persistAcrossSessions: true,
    },
  ];
}

/**
 * Make the registered scripts match the granted origins.
 *
 * Written as replace rather than diff because `matches` cannot be edited in
 * place and the set is small. Unregistering by id first keeps this safe to call
 * repeatedly, which matters: it runs on startup, on install, and on every
 * grant or revoke.
 */
export async function syncRegistrations(scripting: ScriptingArea, origins: string[]): Promise<void> {
  const ours = [RELAY_ID, COMPAT_ID];
  const existing = await scripting.getRegisteredContentScripts({ ids: ours });
  if (existing.length > 0) {
    await scripting.unregisterContentScripts({ ids: existing.map((script) => script.id) });
  }

  const scripts = scriptsFor(origins);
  if (scripts.length > 0) await scripting.registerContentScripts(scripts);
}

/**
 * Origins the user has granted, as match patterns.
 *
 * `chrome.permissions.getAll()` reports the wildcard entries from
 * `optional_host_permissions` too if they were ever granted wholesale, and our
 * own named hosts. Neither belongs here: the first would put us back on every
 * site, and the second is the daemon and our API, which are not pages.
 */
export function grantedOrigins(patterns: string[], declaredHosts: string[]): string[] {
  const declared = new Set(declaredHosts);
  return patterns.filter((pattern) => {
    if (declared.has(pattern)) return false;
    const host = pattern.split("://")[1]?.split("/")[0] ?? "";
    return host !== "" && !host.startsWith("*");
  });
}
