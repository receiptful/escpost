import { extensionProtocolVersion, isPageRequest, type ErrorCode, type PageRequest, type WorkerReply } from "./protocol";
import { DaemonError } from "./daemon";
import { isDaemonOrigin, originPattern } from "./registration";

const maximumRawJobBytes = 8 * 1024 * 1024;
const base64Alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

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
  if (pattern === null || isDaemonOrigin(senderOrigin ?? "") || !await granted(pattern, deps)) {
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
  } catch (error) {
    if (error instanceof DaemonError && error.code === "PRINTER_NOT_FOUND") {
      return failure("PRINTER_NOT_FOUND", error.message);
    }
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
  if (!isRecord(value)
    || Object.keys(value).length !== 2
    || !Object.hasOwn(value, "printer")
    || !Object.hasOwn(value, "dataBase64")
    || typeof value.printer !== "string"
    || value.printer.length === 0
    || typeof value.dataBase64 !== "string") {
    return null;
  }
  const decodedLength = decodedBase64Length(value.dataBase64);
  if (decodedLength === null || decodedLength > maximumRawJobBytes) return null;
  try {
    const binary = atob(value.dataBase64);
    if (binary.length !== decodedLength || binary.length > maximumRawJobBytes) return null;
    return { printer: value.printer, bytes: Uint8Array.from(binary, (character) => character.charCodeAt(0)) };
  } catch {
    return null;
  }
}

function isPaddedBase64(value: string): boolean {
  if (value.length % 4 !== 0) return false;
  const padding = value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0;
  const contentLength = value.length - padding;
  if ((padding === 2 && contentLength % 4 !== 2) || (padding === 1 && contentLength % 4 !== 3)) return false;
  for (let index = 0; index < contentLength; index += 1) {
    if (base64Alphabet.indexOf(value[index]) === -1) return false;
  }
  if (padding === 2) return (base64Alphabet.indexOf(value[contentLength - 1]) & 0x0f) === 0;
  if (padding === 1) return (base64Alphabet.indexOf(value[contentLength - 1]) & 0x03) === 0;
  return true;
}

function decodedBase64Length(value: string): number | null {
  if (!isPaddedBase64(value)) return null;
  const padding = value.endsWith("==") ? 2 : value.endsWith("=") ? 1 : 0;
  return (value.length / 4) * 3 - padding;
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
