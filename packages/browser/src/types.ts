export interface Printer {
  id: string;
  name: string;
  transport: "usb" | "network";
  profile: string | null;
  status: "ready" | "unavailable";
}

/**
 * Where a job is printed, as opposed to `Printer.transport`, which is how a
 * printer is wired. Only "local" exists today; a union of one, so adding
 * "cloud" later is not an API break.
 */
export type PrintTarget = "local";

export interface RawPrintRequest {
  printer: string;
  data: Uint8Array | string;
  html?: never;
  target?: PrintTarget;
}

export interface HtmlPrintRequest {
  printer: string;
  html: string;
  data?: never;
  target?: PrintTarget;
}

export type PrintRequest = RawPrintRequest | HtmlPrintRequest;

export interface PrintResult {
  jobId: string;
}
