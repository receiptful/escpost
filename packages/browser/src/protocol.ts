/** Bumped when the page<->worker message shape changes incompatibly. */
export const PROTOCOL_VERSION = 1;

export const PAGE_SOURCE = "escpost-page";
export const EXT_SOURCE = "escpost-ext";

export interface PageMessage {
  source: typeof PAGE_SOURCE;
  id: number;
  protocol: number;
  op: string;
  payload: unknown;
}

export interface ExtMessage {
  source: typeof EXT_SOURCE;
  id: number;
  ok: boolean;
  data?: unknown;
  error?: { code: string; message: string };
}
