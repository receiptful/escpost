import { extensionProtocolVersion, isPageRequest, type ErrorCode, type PageRequest, type WorkerReply } from "./protocol";
import { originPattern } from "./registration";

type OneShotDaemon = {
  health(): Promise<unknown>;
  list(transport?: "usb" | "network"): Promise<unknown>;
  print(printer: string, bytes: Uint8Array): Promise<unknown>;
};

export type RequestDependencies = {
  permissions: { contains(details: { origins: string[] }): Promise<boolean> };
  daemon: OneShotDaemon;
};

export async function handleRequest(
  request: unknown,
  senderOrigin: string | undefined,
  deps: RequestDependencies,
): Promise<WorkerReply> {
  const pattern = originPattern(senderOrigin);
  if (pattern === null || !await granted(pattern, deps)) {
    return failure("ORIGIN_NOT_GRANTED", "This page origin is not granted access to ESCPost.");
  }
  if (!isPageRequest(request) || request.protocol !== extensionProtocolVersion) {
    return failure("PROTOCOL_MISMATCH", "The page request does not match the ESCPost protocol.");
  }

  switch (request.op) {
    case "daemon.health":
      return request.payload === null ? daemonCall(() => deps.daemon.health()) : protocolFailure();
    case "printers.list":
      return listRequest(request, deps);
    case "print.raw":
      return printRequest(request, deps);
    default:
      return protocolFailure();
  }
}

async function granted(pattern: string, deps: RequestDependencies): Promise<boolean> {
  try {
    return await deps.permissions.contains({ origins: [pattern] });
  } catch {
    return false;
  }
}

async function listRequest(request: PageRequest, deps: RequestDependencies): Promise<WorkerReply> {
  const payload = request.payload;
  if (!isListPayload(payload)) return protocolFailure();
  return daemonCall(() => deps.daemon.list(payload.transport));
}

async function printRequest(request: PageRequest, deps: RequestDependencies): Promise<WorkerReply> {
  const payload = decodeRawPrint(request.payload);
  if (payload === null) return protocolFailure();
  try {
    return { ok: true, data: await deps.daemon.print(payload.printer, payload.bytes) };
  } catch {
    return failure("PRINT_FAILED", "The daemon could not complete the raw print job.");
  }
}

async function daemonCall(call: () => Promise<unknown>): Promise<WorkerReply> {
  try {
    return { ok: true, data: await call() };
  } catch {
    return failure("DAEMON_UNAVAILABLE", "The local ESCPost daemon is unavailable.");
  }
}

function isListPayload(value: unknown): value is { transport?: "usb" | "network" } {
  if (!isRecord(value)) return false;
  return Object.keys(value).every((key) => key === "transport")
    && (value.transport === undefined || value.transport === "usb" || value.transport === "network");
}

function decodeRawPrint(value: unknown): { printer: string; bytes: Uint8Array } | null {
  if (!isRecord(value) || typeof value.printer !== "string" || value.printer.length === 0 || typeof value.dataBase64 !== "string") {
    return null;
  }
  if (!isPaddedBase64(value.dataBase64)) return null;
  try {
    const binary = atob(value.dataBase64);
    return { printer: value.printer, bytes: Uint8Array.from(binary, (character) => character.charCodeAt(0)) };
  } catch {
    return null;
  }
}

function isPaddedBase64(value: string): boolean {
  return /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value);
}

function protocolFailure(): WorkerReply {
  return failure("PROTOCOL_MISMATCH", "The page request payload is invalid.");
}

function failure(code: ErrorCode, message: string): WorkerReply {
  return { ok: false, error: { code, message } };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
