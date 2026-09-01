import type { RawPrintPayload } from "./protocol";
import { PageTransport, type PageWindow } from "./transport";
import type {
  NetworkConnection,
  Printer,
  PrinterInventory,
  PrintResult,
  RawPrintRequest,
  UsbConnection,
} from "./types";

export { EscpostError } from "./errors";
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
