import type { EscpostError } from "./errors";
import type { RawPrintPayload } from "./protocol";
import { PageTransport, type PageWindow } from "./transport";
import { SubscriptionTransport } from "./subscriptions";
import type {
  NetworkConnection,
  Printer,
  PrinterInventory,
  PrintResult,
  RawPrintRequest,
  UsbConnection,
} from "./types";

export { EscpostError } from "./errors";
export type { ErrorCode } from "./errors";
export type {
  NetworkConnection,
  Printer,
  PrinterInventory,
  PrintResult,
  RawPrintRequest,
  UsbConnection,
} from "./types";

const healthTimeoutMs = 2_000;
const listTimeoutMs = 30_000;
const printTimeoutMs = 20_000;

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

type WirePrinterInventory = {
  updated_at: string;
  warning: string | null;
  printers: WirePrinter[];
};

type WirePrintResult = { job_id: string };

function createEscpostClient(page?: PageWindow) {
  const transport = new PageTransport(page);
  const subscriptions = new SubscriptionTransport(page);

  return {
    async isAvailable(): Promise<boolean> {
      try {
        await transport.request("daemon.health", null, healthTimeoutMs);
        return true;
      } catch {
        return false;
      }
    },

    printers: {
      async list(options: { transport?: "usb" | "network" } = {}): Promise<PrinterInventory> {
        const snapshot = await transport.request<WirePrinterInventory>(
          "printers.list",
          options.transport === undefined ? {} : { transport: options.transport },
          listTimeoutMs,
        );
        return mapInventory(snapshot);
      },

      subscribe(
        onSnapshot: (snapshot: PrinterInventory) => void,
        options: { onError?: (error: EscpostError) => void } = {},
      ): () => void {
        return subscriptions.subscribe<WirePrinterInventory>(
          (snapshot) => onSnapshot(mapInventory(snapshot)),
          options,
          isWirePrinterInventory,
        );
      },
    },

    async print(request: RawPrintRequest): Promise<PrintResult> {
      const payload: RawPrintPayload = {
        printer: request.printer,
        dataBase64: encodeBase64(
          typeof request.data === "string" ? new TextEncoder().encode(request.data) : request.data,
        ),
      };
      const result = await transport.request<WirePrintResult>("print.raw", payload, printTimeoutMs);
      return { jobId: result.job_id };
    },
  };
}

export const escpost = createEscpostClient();

function isWirePrinterInventory(value: unknown): value is WirePrinterInventory {
  if (!isRecord(value)) return false;
  return (
    typeof value.updated_at === "string" &&
    isNullableString(value.warning) &&
    Array.isArray(value.printers) &&
    value.printers.every(isWirePrinter)
  );
}

function isWirePrinter(value: unknown): value is WirePrinter {
  if (!isRecord(value)) return false;
  return (
    typeof value.name === "string" &&
    (value.transport === "usb" || value.transport === "network") &&
    (value.availability === "connected" || value.availability === "unavailable") &&
    isNullableString(value.profile) &&
    isWireConnection(value.connection)
  );
}

function isWireConnection(value: unknown): value is WireConnection {
  if (!isRecord(value) || typeof value.type !== "string") return false;
  if (value.type === "network") {
    return typeof value.host === "string" && isNumber(value.port);
  }
  return (
    value.type === "usb" &&
    isNumber(value.vendor_id) &&
    isNumber(value.product_id) &&
    isNullableString(value.bus) &&
    isNullableNumber(value.address) &&
    isNullableString(value.manufacturer) &&
    isNullableString(value.product) &&
    isNullableString(value.serial_number) &&
    isNumber(value.interface_number) &&
    isNumberArray(value.out_endpoints) &&
    isNumberArray(value.in_endpoints)
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isNullableString(value: unknown): value is string | null {
  return typeof value === "string" || value === null;
}

function isNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function isNullableNumber(value: unknown): value is number | null {
  return value === null || isNumber(value);
}

function isNumberArray(value: unknown): value is number[] {
  return Array.isArray(value) && value.every(isNumber);
}

function mapInventory(snapshot: WirePrinterInventory): PrinterInventory {
  return {
    updatedAt: snapshot.updated_at,
    warning: snapshot.warning,
    printers: snapshot.printers.map(mapPrinter),
  };
}

function mapPrinter(printer: WirePrinter): Printer {
  return {
    name: printer.name,
    transport: printer.transport,
    availability: printer.availability,
    profile: printer.profile,
    connection:
      printer.connection.type === "usb"
        ? {
            type: "usb",
            vendorId: printer.connection.vendor_id,
            productId: printer.connection.product_id,
            bus: printer.connection.bus,
            address: printer.connection.address,
            manufacturer: printer.connection.manufacturer,
            product: printer.connection.product,
            serialNumber: printer.connection.serial_number,
            interfaceNumber: printer.connection.interface_number,
            outEndpoints: printer.connection.out_endpoints,
            inEndpoints: printer.connection.in_endpoints,
          }
        : {
            type: "network",
            host: printer.connection.host,
            port: printer.connection.port,
          },
  };
}

function encodeBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let index = 0; index < bytes.length; index += 1) {
    binary += String.fromCharCode(bytes[index]);
  }
  return btoa(binary);
}
