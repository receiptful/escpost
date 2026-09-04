export const extensionProtocolVersion = 1;

export type PageRequest = {
  source: "escpost-page";
  protocol: number;
  id: number;
  op: string;
  payload: unknown;
};

export type RelayRequest = {
  source: "escpost-relay";
  request: PageRequest;
};

export type WorkerReply =
  | { ok: true; data: unknown }
  | { ok: false; error: { code: ErrorCode; message: string } };

export type PageReply = WorkerReply & { source: "escpost-extension"; id: number };

export type RelayProbeRequest = {
  source: "escpost-popup";
  kind: "relay-probe";
  protocol: number;
};

export type RelayProbeReply = {
  source: "escpost-popup";
  kind: "relay-probe-result";
  protocol: number;
  relay: true;
  daemon: boolean | null;
};

export type ErrorCode =
  | "EXTENSION_UNAVAILABLE"
  | "ORIGIN_NOT_GRANTED"
  | "DAEMON_UNAVAILABLE"
  | "PRINTER_NOT_FOUND"
  | "PRINT_FAILED"
  | "PROTOCOL_MISMATCH";

export function isPageRequest(value: unknown): value is PageRequest {
  if (!isRecord(value)) return false;
  return value.source === "escpost-page"
    && typeof value.protocol === "number"
    && Number.isSafeInteger(value.id)
    && typeof value.op === "string"
    && "payload" in value;
}

export function isRelayRequest(value: unknown): value is RelayRequest {
  return isRecord(value) && value.source === "escpost-relay" && isPageRequest(value.request);
}

export function isRelayProbeRequest(value: unknown): value is RelayProbeRequest {
  return isRecord(value)
    && hasExactOwnKeys(value, ["source", "kind", "protocol"])
    && value.source === "escpost-popup"
    && value.kind === "relay-probe"
    && value.protocol === extensionProtocolVersion;
}

export function isRelayProbeReply(value: unknown): value is RelayProbeReply {
  return isRecord(value)
    && hasExactOwnKeys(value, ["source", "kind", "protocol", "relay", "daemon"])
    && value.source === "escpost-popup"
    && value.kind === "relay-probe-result"
    && value.protocol === extensionProtocolVersion
    && value.relay === true
    && (typeof value.daemon === "boolean" || value.daemon === null);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasExactOwnKeys(value: Record<string, unknown>, keys: string[]): boolean {
  return Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}
