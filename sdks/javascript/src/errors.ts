export type ErrorCode =
  | "EXTENSION_UNAVAILABLE"
  | "ORIGIN_NOT_GRANTED"
  | "DAEMON_UNAVAILABLE"
  | "PRINTER_NOT_FOUND"
  | "PRINT_FAILED"
  | "PROTOCOL_MISMATCH";

const errorCodes: ReadonlySet<ErrorCode> = new Set([
  "EXTENSION_UNAVAILABLE",
  "ORIGIN_NOT_GRANTED",
  "DAEMON_UNAVAILABLE",
  "PRINTER_NOT_FOUND",
  "PRINT_FAILED",
  "PROTOCOL_MISMATCH",
]);

export class EscpostError extends Error {
  readonly code: ErrorCode;

  constructor(code: ErrorCode, message: string) {
    super(message);
    this.name = "EscpostError";
    this.code = code;
  }
}

export function fromSerializedError(value: unknown): EscpostError {
  if (!isSerializedError(value)) {
    return new EscpostError("PROTOCOL_MISMATCH", "The extension returned an invalid error.");
  }

  return new EscpostError(value.code, value.message);
}

function isSerializedError(value: unknown): value is { code: ErrorCode; message: string } {
  if (typeof value !== "object" || value === null) return false;
  const error = value as { code?: unknown; message?: unknown };
  return typeof error.code === "string" && errorCodes.has(error.code as ErrorCode) && typeof error.message === "string";
}
