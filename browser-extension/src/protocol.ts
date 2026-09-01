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

export type ErrorCode =
  | "ORIGIN_NOT_GRANTED"
  | "DAEMON_UNAVAILABLE"
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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
