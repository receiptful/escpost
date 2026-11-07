export type Printer = {
  name: string;
  transport: "usb" | "network";
  availability: "connected" | "unavailable";
  profile: string | null;
  connection: UsbConnection | NetworkConnection;
};

export type UsbConnection = {
  type: "usb";
  vendorId: number;
  productId: number;
  bus: string | null;
  address: number | null;
  manufacturer: string | null;
  product: string | null;
  serialNumber: string | null;
  interfaceNumber: number;
  outEndpoints: number[];
  inEndpoints: number[];
};

export type NetworkConnection = {
  type: "network";
  host: string;
  port: number;
};

export type PrinterInventory = {
  updatedAt: string;
  warning: string | null;
  printers: Printer[];
};

/**
 * A printer model's facts from ESCPost's profile catalog. A configured printer
 * names its profile in `Printer.profile`. `printableWidthDots`, not the paper
 * width, is the width a print head can print: 80 mm paper prints 512 dots on a
 * 180 dpi TM-T88III and 576 on a 203 dpi head.
 */
export type PrinterProfile = {
  id: string;
  vendor: string;
  model: string;
  source: "calibrated" | "synthesized" | "virtual";
  paperWidthMm: number;
  printableWidthMm: number;
  printableWidthDots: number;
  dpiX: number;
  dpiY: number;
  fullCut: boolean;
  partialCut: boolean;
  barcodeFunctionA: boolean;
  barcodeFunctionB: boolean;
  qrCode: boolean;
};

export type ProfileCatalog = {
  profiles: PrinterProfile[];
};

export type RawPrintRequest = {
  printer: string;
  data: Uint8Array | string;
};

export type PrintResult = { jobId: string };
