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

export async function registerGrantedRelay(deps: RegistrationDependencies): Promise<void> {
  const grants = await deps.permissions.getAll();
  const matches = (grants.origins ?? []).filter(isExplicitWebGrant);
  const existing = await deps.scripting.getRegisteredContentScripts({ ids: [relayId] });

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
  void registerGrantedRelay(deps);
  deps.permissions.onAdded?.addListener(() => { void registerGrantedRelay(deps); });
  deps.permissions.onRemoved?.addListener(() => { void registerGrantedRelay(deps); });
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

function isExplicitWebGrant(pattern: string): boolean {
  const origin = originPattern(pattern);
  if (origin !== pattern) return false;
  try {
    const url = new URL(pattern);
    return !(url.hostname === DAEMON_HOST && DAEMON_PORTS.some((port) => port === Number(url.port)));
  } catch {
    return false;
  }
}
