export type RelayStatus = {
  relay: "loaded" | "missing" | "unknown";
  daemon: "running" | "unavailable" | "unknown";
  error: string | null;
};

export type RelayProbeScripting = {
  executeScript(details: {
    target: { tabId: number };
    func: (id: number, origin: string) => Promise<unknown>;
    args: [number, string];
  }): Promise<Array<{ result?: unknown }>>;
};

let nextRequestId = 1;

export async function probeRelayStatus(
  tabId: number,
  origin: string,
  scripting: RelayProbeScripting,
): Promise<RelayStatus> {
  try {
    const results = await scripting.executeScript({
      target: { tabId },
      func: requestRelayHealth,
      args: [nextProbeId(), origin],
    });
    return parseProbeResult(results[0]?.result);
  } catch {
    return { relay: "unknown", daemon: "unknown", error: "Could not contact the page relay." };
  }
}

function nextProbeId(): number {
  const id = nextRequestId;
  nextRequestId = nextRequestId === Number.MAX_SAFE_INTEGER ? 1 : nextRequestId + 1;
  return id;
}

function parseProbeResult(value: unknown): RelayStatus {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { relay: "missing", daemon: "unknown", error: null };
  }
  const result = value as { relay?: unknown; daemon?: unknown };
  if (result.relay !== true) return { relay: "missing", daemon: "unknown", error: null };
  if (result.daemon === true) return { relay: "loaded", daemon: "running", error: null };
  if (result.daemon === false) return { relay: "loaded", daemon: "unavailable", error: null };
  return { relay: "loaded", daemon: "unknown", error: null };
}

function requestRelayHealth(id: number, origin: string): Promise<unknown> {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (result: unknown) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      window.removeEventListener("message", receive);
      resolve(result);
    };
    const receive = (event: MessageEvent) => {
      if (event.source !== window || event.origin !== origin) return;
      const reply = event.data;
      if (typeof reply !== "object" || reply === null || Array.isArray(reply)) return;
      const candidate = reply as { source?: unknown; id?: unknown; ok?: unknown; data?: unknown };
      if (candidate.source !== "escpost-extension" || candidate.id !== id) return;
      finish({ relay: true, daemon: candidate.ok === true && typeof candidate.data === "boolean" ? candidate.data : null });
    };
    const timeout = setTimeout(() => finish({ relay: false }), 250);
    window.addEventListener("message", receive);
    window.postMessage({ source: "escpost-page", protocol: extensionProtocolVersion, id, op: "daemon.health", payload: null }, origin);
  });
}
import { extensionProtocolVersion } from "../protocol";
