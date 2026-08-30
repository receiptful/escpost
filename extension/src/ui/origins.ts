// The daemon's own host permission comes back from chrome.permissions.getAll()
// alongside every site the user granted. It is not a site that can print and must
// never be counted as one, or the popup's footer will always read one too many.
//
// `localhost` is deliberately NOT in this set: the daemon is declared at
// http://127.0.0.1, and someone's own dev server on http://localhost:3000 is a real
// site they granted and must be able to revoke.
const LOCAL_HOSTS = new Set(["127.0.0.1", "0.0.0.0", "[::1]"]);

/**
 * Turns a host permission pattern into something to show a person, or null to
 * hide it.
 *
 * `declared` is the manifest's own `host_permissions`: the daemon and our API.
 * Those are infrastructure the extension needs, not sites the user granted, and
 * listing them under "Sites that can print" offers a Revoke link that would
 * quietly break HTML rendering for something that never printed anything.
 */
export function displayOrigin(pattern: string, declared: readonly string[] = []): string | null {
  if (declared.includes(pattern)) return null;

  const match = /^(https?):\/\/([^/]+)\/\*$/.exec(pattern);
  if (match === null) return null;

  const scheme = match[1];
  const host = match[2];
  if (scheme === undefined || host === undefined) return null;

  // Granted through Chrome's own "On all sites" control. Hiding it would show an
  // empty site list to someone every site can print from.
  if (host === "*") return `Every ${scheme} site`;

  // "[::1]:9000" must not be split on its colons the way "127.0.0.1:9000" is.
  const bare = host.startsWith("[") ? host.slice(0, host.indexOf("]") + 1) : (host.split(":")[0] ?? "");
  if (LOCAL_HOSTS.has(bare)) return null;

  return scheme === "https" ? host : `${scheme}://${host}`;
}

export function isWebOrigin(pattern: string, declared: readonly string[] = []): boolean {
  return displayOrigin(pattern, declared) !== null;
}
