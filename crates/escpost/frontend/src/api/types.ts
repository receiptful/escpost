export type ApiErrorEnvelope = {
  error: {
    code: string;
    message: string;
  };
};

export type VirtualPrinterStatus = {
  state: "ready" | "receiving";
  address: string;
};

export type StatusResponse = {
  virtual_printer: VirtualPrinterStatus | null;
  jobs_processed: number;
};

export type UsbConnection = {
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
};

export type NetworkConnection = {
  type: "network";
  host: string;
  port: number;
};

export type Printer = {
  name: string;
  transport: "usb" | "network";
  availability: "connected" | "unavailable";
  profile: string | null;
  connection: UsbConnection | NetworkConnection;
};

export type PrintersResponse = {
  printers: Printer[];
};

export type Profile = {
  id: string;
  vendor: string;
  model: string;
  source: "calibrated" | "synthesized" | "virtual";
  paper_width_mm: number;
  printable_width_mm: number;
  printable_width_dots: number;
  dpi_x: number;
  dpi_y: number;
  full_cut: boolean;
  partial_cut: boolean;
  barcode_function_a: boolean;
  barcode_function_b: boolean;
  qr_code: boolean;
};

export type ProfilesResponse = {
  profiles: Profile[];
};

export type Position = { x: number; y: number };
export type Region = { x: number; y: number; width: number; height: number };

export type CommandEffect =
  | { type: "state_change"; state: string; before: string; after: string }
  | { type: "motion"; before: Position; after: Position }
  | { type: "paint"; bounds: Region };

export type JobCommand = {
  byte_start: number;
  byte_end: number;
  name: string;
  detail: string;
  paint_lifecycle?: "buffered" | "committed";
  annotation?: { label: string; content: string };
  effects: CommandEffect[];
};

export type JobSheet = {
  number: number;
  name: string;
  width_dots?: number;
  height_dots?: number;
  image_url?: string;
  commands: JobCommand[];
};

export type CurrentJob = {
  id: string;
  completed_at_unix_ms?: number;
  completion?: "closed" | "timeout";
  antialias: boolean;
  warnings: string[];
  input_url?: string;
  sheets: JobSheet[];
};

export type CurrentJobResponse = {
  receiving: boolean;
  profile: string;
  error: string | null;
  hint?: string;
  job: CurrentJob | null;
};
