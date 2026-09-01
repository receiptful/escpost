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

export type RawPrintRequest = {
  printer: string;
  data: Uint8Array | string;
};

export type PrintResult = { jobId: string };
