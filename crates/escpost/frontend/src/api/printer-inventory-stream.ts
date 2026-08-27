import type { PrintersResponse } from "./types";

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isUnsignedInteger(value: unknown, maximum: number): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 && value <= maximum;
}

function isNumberArray(value: unknown): value is number[] {
  return Array.isArray(value) && value.every((entry) => isUnsignedInteger(entry, 0xff));
}

function isRfc3339(value: unknown): value is string {
  return typeof value === "string"
    && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/.test(value)
    && !Number.isNaN(Date.parse(value));
}

function isNetworkConnection(value: unknown): boolean {
  return isObject(value)
    && value.type === "network"
    && typeof value.host === "string"
    && isUnsignedInteger(value.port, 0xffff);
}

function isUsbConnection(value: unknown): boolean {
  return isObject(value)
    && value.type === "usb"
    && isUnsignedInteger(value.vendor_id, 0xffff)
    && isUnsignedInteger(value.product_id, 0xffff)
    && isNullableString(value.bus)
    && (value.address === null || isUnsignedInteger(value.address, 0xff))
    && isNullableString(value.manufacturer)
    && isNullableString(value.product)
    && isNullableString(value.serial_number)
    && isUnsignedInteger(value.interface_number, 0xff)
    && isNumberArray(value.out_endpoints)
    && isNumberArray(value.in_endpoints);
}

function isPrinter(value: unknown): boolean {
  if (!isObject(value)
    || typeof value.name !== "string"
    || (value.transport !== "usb" && value.transport !== "network")
    || (value.availability !== "connected" && value.availability !== "unavailable")
    || !isNullableString(value.profile)) {
    return false;
  }
  return value.transport === "network" ? isNetworkConnection(value.connection) : isUsbConnection(value.connection);
}

function isPrintersResponse(value: unknown): value is PrintersResponse {
  return isObject(value)
    && isRfc3339(value.updated_at)
    && isNullableString(value.warning)
    && Array.isArray(value.printers)
    && value.printers.every(isPrinter);
}

export function openPrinterInventoryStream(callbacks: {
  onSnapshot: (snapshot: PrintersResponse) => void;
  onError: (error: Error) => void;
}): () => void {
  const source = new EventSource("/api/printers/list/events");
  source.addEventListener("message", (event) => {
    try {
      const value: unknown = JSON.parse((event as MessageEvent<string>).data);
      if (!isPrintersResponse(value)) {
        throw new Error("The server sent an invalid printer inventory.");
      }
      callbacks.onSnapshot(value);
    } catch {
      callbacks.onError(new Error("The server sent an invalid printer inventory."));
    }
  });
  source.addEventListener("error", () => {
    callbacks.onError(new Error("Printer monitoring disconnected; retrying automatically."));
  });
  return () => source.close();
}
