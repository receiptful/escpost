import { DAEMON_HOST, DAEMON_PORTS } from "./config";

const relayId = "escpost-relay";

type ContentScript = {
  id: string;
  js: string[];
  matches: string[];
  runAt: "document_start";
  world: "ISOLATED";
};

type RegisteredScript = { id: string };

export type RegistrationDependencies = {
  permissions: {
    getAll(): Promise<{ origins?: string[] }>;
    onAdded?: { addListener(listener: () => void): void };
    onRemoved?: { addListener(listener: () => void): void };
  };
  scripting: {
    getRegisteredContentScripts(details: { ids: string[] }): Promise<RegisteredScript[]>;
    registerContentScripts(scripts: ContentScript[]): Promise<void>;
    updateContentScripts(scripts: ContentScript[]): Promise<void>;
    unregisterContentScripts(details: { ids: string[] }): Promise<void>;
  };
};

export async function registerGrantedRelay(
  deps: RegistrationDependencies,
  isCurrent: () => boolean = () => true,
): Promise<void> {
  const grants = await deps.permissions.getAll();
  if (!isCurrent()) return;
  const matches = (grants.origins ?? []).filter(isExplicitWebGrant);
  const existing = await deps.scripting.getRegisteredContentScripts({ ids: [relayId] });
  if (!isCurrent()) return;

  if (matches.length === 0) {
    if (existing.length > 0) await deps.scripting.unregisterContentScripts({ ids: [relayId] });
    return;
  }

  const script: ContentScript = {
    id: relayId,
    js: ["relay.js"],
    matches,
    runAt: "document_start",
    world: "ISOLATED",
  };
  if (existing.length === 0) {
    await deps.scripting.registerContentScripts([script]);
  } else {
    await deps.scripting.updateContentScripts([script]);
  }
}

export function installGrantRegistration(deps: RegistrationDependencies): void {
  let revision = 0;
  let refreshing = false;
  const refresh = () => {
    revision += 1;
    if (!refreshing) void drain();
  };
  const drain = async () => {
    refreshing = true;
    try {
      let observed: number;
      do {
        observed = revision;
        await registerGrantedRelay(deps, () => revision === observed);
      } while (observed !== revision);
    } catch {
      // A later Chrome permission event starts a fresh pass. Retrying here
      // would spin indefinitely when the API remains unavailable.
    } finally {
      refreshing = false;
    }
  };

  refresh();
  deps.permissions.onAdded?.addListener(refresh);
  deps.permissions.onRemoved?.addListener(refresh);
}

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

function isExplicitWebGrant(pattern: string): boolean {
  const origin = originPattern(pattern);
  if (origin !== pattern) return false;
  try {
    const url = new URL(pattern);
    return !url.hostname.includes("*") && !isDaemonOrigin(pattern);
  } catch {
    return false;
  }
}
