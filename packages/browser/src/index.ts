import { EscpostError } from "./errors";
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
        const available = await transport.request<unknown>("daemon.health", null, healthTimeoutMs);
        if (typeof available !== "boolean") {
          throw protocolMismatch("health result");
        }
        return available;
      } catch {
        return false;
      }
    },

    printers: {
      async list(options: { transport?: "usb" | "network" } = {}): Promise<PrinterInventory> {
        const snapshot = await transport.request<unknown>(
          "printers.list",
          options.transport === undefined ? {} : { transport: options.transport },
          listTimeoutMs,
        );
        if (!isWirePrinterInventory(snapshot)) {
          throw protocolMismatch("printer inventory");
        }
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
      const result = await transport.request<unknown>("print.raw", payload, printTimeoutMs);
      if (!isWirePrintResult(result)) {
        throw protocolMismatch("print result");
      }
      return { jobId: result.job_id };
    },
  };
}

export const escpost = createEscpostClient();

function isWirePrinterInventory(value: unknown): value is WirePrinterInventory {
  if (!isRecord(value)) return false;
  return (
    isRfc3339(value.updated_at) &&
    isNullableString(value.warning) &&
    Array.isArray(value.printers) &&
    value.printers.every(isWirePrinter)
  );
}

function isWirePrintResult(value: unknown): value is WirePrintResult {
  return isRecord(value) && typeof value.job_id === "string";
}

function protocolMismatch(resultName: string): EscpostError {
  return new EscpostError("PROTOCOL_MISMATCH", `The extension returned an invalid ${resultName}.`);
}

function isWirePrinter(value: unknown): value is WirePrinter {
  if (!isRecord(value)) return false;
  return (
    typeof value.name === "string" &&
    (value.transport === "usb" || value.transport === "network") &&
    (value.availability === "connected" || value.availability === "unavailable") &&
    isNullableString(value.profile) &&
    (value.transport === "network" ? isNetworkConnection(value.connection) : isUsbConnection(value.connection))
  );
}

function isNetworkConnection(value: unknown): boolean {
  return isRecord(value)
    && value.type === "network"
    && typeof value.host === "string"
    && isUnsignedInteger(value.port, 0xffff);
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
