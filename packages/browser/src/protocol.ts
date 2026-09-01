import type { ErrorCode } from "./errors";

export const protocolVersion = 1;

export type PageOperation = "daemon.health" | "printers.list" | "print.raw";

export type PageRequest = {
  source: "escpost-page";
  protocol: 1;
  id: number;
  op: PageOperation;
  payload: unknown;
};

export type RawPrintPayload = {
  printer: string;
  dataBase64: string;
};

export type SerializedError = {
  code: ErrorCode;
  message: string;
};

export type ExtensionReply =
  | { source: "escpost-extension"; id: number; ok: true; data: unknown }
  | { source: "escpost-extension"; id: number; ok: false; error: SerializedError };
