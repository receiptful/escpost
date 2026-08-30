export const ERROR_CODES = [
  "ORIGIN_NOT_GRANTED",
  "DAEMON_NOT_RUNNING",
  "PRINTER_NOT_FOUND",
  "RENDER_FAILED",
  "RENDER_UNAVAILABLE",
  "UNSUPPORTED_FORMAT",
  "PRINT_FAILED",
  "NOT_SIGNED_IN",
  "QUOTA_EXCEEDED",
  "EXTENSION_NOT_INSTALLED",
  "VERSION_MISMATCH",
] as const;

export type ErrorCode = (typeof ERROR_CODES)[number];

/** Codes where the failure must never read as "the extension is broken". */
const RAW_UNAFFECTED: ReadonlySet<ErrorCode> = new Set(["RENDER_UNAVAILABLE", "QUOTA_EXCEEDED"]);

const RAW_NOTE = " Raw printing is unaffected.";

export class EscpostError extends Error {
  readonly code: ErrorCode;

  constructor(code: ErrorCode, message: string) {
    super(RAW_UNAFFECTED.has(code) && !message.includes(RAW_NOTE.trim()) ? message + RAW_NOTE : message);
    this.code = code;
    this.name = "EscpostError";
  }
}

/** A structured-clone round trip drops the prototype, so identity is by shape. */
export function isEscpostError(value: unknown): value is { code: ErrorCode; message: string } {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as { code?: unknown; message?: unknown };
  return (
    typeof candidate.code === "string" &&
    (ERROR_CODES as readonly string[]).includes(candidate.code) &&
    typeof candidate.message === "string"
  );
}
