import { DAEMON_HOST, DAEMON_PORTS } from "./config";
import { DaemonPortStore } from "./daemon-port";

type WireConnection =
  | {
      type: "usb";
      vendor_id: number;
      product_id: number;
      bus: string | null;
      address: number | null;
      manufacturer: string | null;
      product: string | null;
      serial_number: string | null;
      interface_number: number;
      out_endpoints: number[];
      in_endpoints: number[];
    }
  | { type: "network"; host: string; port: number };

type WirePrinter = {
  name: string;
  transport: "usb" | "network";
  availability: "connected" | "unavailable";
  profile: string | null;
  connection: WireConnection;
};

export type WirePrinterInventory = {
  updated_at: string;
  warning: string | null;
  printers: WirePrinter[];
};

export type InventoryStreamCallbacks = {
  onSnapshot: (snapshot: WirePrinterInventory) => void;
  onError: (error: Error) => void;
};

export class DaemonError extends Error {
  readonly code = "DAEMON_UNAVAILABLE" as const;

  constructor(message: string) {
    super(message);
    this.name = "DaemonError";
  }
}

export class DaemonClient {
  private readonly fetcher: typeof fetch;

  constructor(
    private readonly ports: DaemonPortStore = new DaemonPortStore(),
    fetcher: typeof fetch = globalThis.fetch.bind(globalThis),
  ) {
    this.fetcher = fetcher;
  }

  async health(): Promise<boolean> {
    const cached = await this.ports.read();
    if (cached === null) return (await this.discover()) !== null;

    try {
      const healthy = (await this.fetcher(`${cached}/health`)).ok;
      if (!healthy) await this.ports.invalidate(cached);
      return healthy;
    } catch {
      await this.ports.invalidate(cached);
      return (await this.discover()) !== null;
    }
  }

  async list(transport?: "usb" | "network"): Promise<WirePrinterInventory> {
    const query = transport === undefined ? "" : `?transport=${encodeURIComponent(transport)}`;
    const response = await this.get(`/api/printers/list${query}`);
    const snapshot = await json(response);
    if (!isWirePrinterInventory(snapshot)) {
      throw new DaemonError("The daemon sent an invalid printer inventory.");
    }
    return snapshot;
  }

  async print(printer: string, bytes: Uint8Array): Promise<{ job_id: string }> {
    const baseUrl = await this.baseUrl();
    try {
      const query = new URLSearchParams({ printer });
      const response = await this.fetcher(`${baseUrl}/api/print?${query}`, {
        method: "POST",
        headers: { "Content-Type": "application/octet-stream" },
        body: bytes as unknown as BodyInit,
      });
      if (!response.ok) throw new DaemonError("The daemon could not confirm the print job.");
      const result = await json(response);
      if (!isPrintResult(result)) throw new DaemonError("The daemon sent an invalid print response.");
      return result;
    } catch (error) {
      await this.ports.invalidate(baseUrl);
      if (error instanceof DaemonError) throw error;
      throw new DaemonError("The daemon could not confirm the print job.");
    }
  }

  async openInventoryStream(callbacks: InventoryStreamCallbacks, signal: AbortSignal): Promise<void> {
    if (signal.aborted) return;
    let response: Response;
    try {
      response = await this.get("/api/printers/list/events", signal);
    } catch (error) {
      if (signal.aborted) return;
      callbacks.onError(error instanceof Error ? error : new DaemonError("The daemon is unavailable."));
      return;
    }
    if (signal.aborted) return;
    if (response.body === null) {
      callbacks.onError(new DaemonError("The daemon closed the inventory stream."));
      return;
    }
    await readInventoryEvents(response.body, callbacks, signal);
  }

  private async get(path: string, signal?: AbortSignal): Promise<Response> {
    let baseUrl = await this.baseUrl(signal);
    throwIfAborted(signal);
    try {
      return await this.getAt(baseUrl, path, signal);
    } catch (error) {
      throwIfAborted(signal);
      if (!isTransportError(error)) throw error;
      await this.ports.invalidate(baseUrl);
      throwIfAborted(signal);
      baseUrl = await this.baseUrl(signal);
      throwIfAborted(signal);
      try {
        return await this.getAt(baseUrl, path, signal);
      } catch (retryError) {
        throwIfAborted(signal);
        if (isTransportError(retryError)) await this.ports.invalidate(baseUrl);
        throw unavailable(retryError);
      }
    }
  }

  private async getAt(baseUrl: string, path: string, signal?: AbortSignal): Promise<Response> {
    const response = await this.fetcher(`${baseUrl}${path}`, signal === undefined ? undefined : { signal });
    throwIfAborted(signal);
    if (!response.ok) throw new DaemonError("The daemon rejected the request.");
    return response;
  }

  private async baseUrl(signal?: AbortSignal): Promise<string> {
    const cached = await this.ports.read();
    throwIfAborted(signal);
    return cached ?? await this.discoverOrThrow(signal);
  }

  private async discoverOrThrow(signal?: AbortSignal): Promise<string> {
    const baseUrl = await this.discover(signal);
    if (baseUrl === null) throw new DaemonError("The local daemon is unavailable.");
    return baseUrl;
  }

  private async discover(signal?: AbortSignal): Promise<string | null> {
    for (const port of DAEMON_PORTS) {
      const baseUrl = `http://${DAEMON_HOST}:${port}`;
      try {
        const response = await this.fetcher(`${baseUrl}/health`, signal === undefined ? undefined : { signal });
        throwIfAborted(signal);
        if (!response.ok) continue;
      } catch (error) {
        throwIfAborted(signal);
        continue;
      }
      throwIfAborted(signal);
      await this.ports.remember(baseUrl);
      throwIfAborted(signal);
      return baseUrl;
    }
    return null;
  }
}

function unavailable(error: unknown): DaemonError {
  return error instanceof DaemonError ? error : new DaemonError("The local daemon is unavailable.");
}

function throwIfAborted(signal?: AbortSignal): void {
  if (!signal?.aborted) return;
  throw signal.reason instanceof Error ? signal.reason : new DOMException("Aborted", "AbortError");
}

function isTransportError(error: unknown): boolean {
  return !(error instanceof DaemonError);
}

async function json(response: Response): Promise<unknown> {
  try {
    return await response.json();
  } catch {
    throw new DaemonError("The daemon sent an invalid response.");
  }
}

async function readInventoryEvents(
  body: ReadableStream<Uint8Array>,
  callbacks: InventoryStreamCallbacks,
  signal: AbortSignal,
): Promise<void> {
  const reader = body.getReader();
  if (signal.aborted) {
    try {
      await reader.cancel();
    } finally {
      reader.releaseLock();
    }
    return;
  }
  const decoder = new TextDecoder();
  let buffer = "";
  const cancel = () => { void reader.cancel(); };
  signal.addEventListener("abort", cancel, { once: true });
  try {
    while (!signal.aborted) {
      const next = await reader.read();
      if (next.done) break;
      buffer += decoder.decode(next.value, { stream: true });
      buffer = consumeEvents(buffer, callbacks);
    }
    if (!signal.aborted) consumeEvents(`${buffer}${decoder.decode()}`, callbacks);
  } finally {
    signal.removeEventListener("abort", cancel);
    reader.releaseLock();
  }
}

function consumeEvents(input: string, callbacks: InventoryStreamCallbacks): string {
  let rest = input;
  for (;;) {
    const delimiter = /\r?\n\r?\n/.exec(rest);
    if (delimiter === null || delimiter.index === undefined) return rest;
    emitEvent(rest.slice(0, delimiter.index), callbacks);
    rest = rest.slice(delimiter.index + delimiter[0].length);
  }
}

function emitEvent(block: string, callbacks: InventoryStreamCallbacks): void {
  let event = "message";
  let data: string[] = [];
  for (const line of block.split(/\r?\n/)) {
    if (line.startsWith("event:")) {
      event = line.slice("event:".length).replace(/^ /, "");
    } else if (line.startsWith("data:")) {
      data.push(line.slice("data:".length).replace(/^ /, ""));
    }
  }
  if ((event === "" || event === "message") && data.length > 0) emitSnapshot(data.join("\n"), callbacks);
}

function emitSnapshot(data: string, callbacks: InventoryStreamCallbacks): void {
  try {
    const snapshot: unknown = JSON.parse(data);
    if (!isWirePrinterInventory(snapshot)) throw new Error();
    callbacks.onSnapshot(snapshot);
  } catch {
    callbacks.onError(new DaemonError("The daemon sent an invalid printer inventory."));
  }
}

function isPrintResult(value: unknown): value is { job_id: string } {
  return isRecord(value) && typeof value.job_id === "string";
}

function isWirePrinterInventory(value: unknown): value is WirePrinterInventory {
  return isRecord(value)
    && isRfc3339(value.updated_at)
    && isNullableString(value.warning)
    && Array.isArray(value.printers)
    && value.printers.every(isWirePrinter);
}

function isWirePrinter(value: unknown): value is WirePrinter {
  return isRecord(value)
    && typeof value.name === "string"
    && (value.transport === "usb" || value.transport === "network")
    && (value.availability === "connected" || value.availability === "unavailable")
    && isNullableString(value.profile)
    && (value.transport === "usb" ? isUsbConnection(value.connection) : isNetworkConnection(value.connection));
}

function isNetworkConnection(value: unknown): boolean {
  return isRecord(value) && value.type === "network" && typeof value.host === "string" && isUnsignedInteger(value.port, 0xffff);
}

function isUsbConnection(value: unknown): boolean {
  return isRecord(value)
    && value.type === "usb"
    && isUnsignedInteger(value.vendor_id, 0xffff)
    && isUnsignedInteger(value.product_id, 0xffff)
    && isNullableString(value.bus)
    && (value.address === null || isUnsignedInteger(value.address, 0xff))
    && isNullableString(value.manufacturer)
    && isNullableString(value.product)
    && isNullableString(value.serial_number)
    && isUnsignedInteger(value.interface_number, 0xff)
    && isByteArray(value.out_endpoints)
    && isByteArray(value.in_endpoints);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNullableString(value: unknown): value is string | null {
  return typeof value === "string" || value === null;
}

function isUnsignedInteger(value: unknown, maximum: number): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 && value <= maximum;
}

function isByteArray(value: unknown): value is number[] {
  return Array.isArray(value) && value.every((entry) => isUnsignedInteger(entry, 0xff));
}

function isRfc3339(value: unknown): value is string {
  return typeof value === "string"
    && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.test(value)
    && !Number.isNaN(Date.parse(value));
}
