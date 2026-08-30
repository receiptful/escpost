export const PAGE_SOURCE = "escpost-page";
export const EXT_SOURCE = "escpost-ext";
export const PROTOCOL_VERSION = 1;

export interface WorkerRequest {
  op: string;
  payload: unknown;
  protocol?: number;
}

export type WorkerResponse =
  | { ok: true; data: unknown }
  | { ok: false; error: { code: string; message: string } };
