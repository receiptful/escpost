import { extensionProtocolVersion, isRelayProbeReply } from "../protocol";

export type RelayStatus = {
  relay: "loaded" | "missing" | "unknown";
  daemon: "running" | "unavailable" | "unknown";
  error: string | null;
};

export type RelayProbeTabs = {
  sendMessage(tabId: number, message: unknown): Promise<unknown>;
};

export async function probeRelayStatus(tabId: number, tabs: RelayProbeTabs): Promise<RelayStatus> {
  try {
    return parseProbeResult(await tabs.sendMessage(tabId, {
      source: "escpost-popup",
      kind: "relay-probe",
      protocol: extensionProtocolVersion,
    }));
  } catch (error) {
    if (isMissingRelay(error)) return { relay: "missing", daemon: "unknown", error: null };
    return { relay: "unknown", daemon: "unknown", error: "Could not contact the page relay." };
  }
}

function parseProbeResult(value: unknown): RelayStatus {
  if (!isRelayProbeReply(value)) return { relay: "unknown", daemon: "unknown", error: "Could not contact the page relay." };
  if (value.daemon === true) return { relay: "loaded", daemon: "running", error: null };
  if (value.daemon === false) return { relay: "loaded", daemon: "unavailable", error: null };
  return { relay: "loaded", daemon: "unknown", error: null };
}

function isMissingRelay(error: unknown): boolean {
  return error instanceof Error && error.message.includes("Receiving end does not exist");
}
