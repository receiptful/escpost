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

export type ServerStatusSnapshot = {
  virtual_printer: VirtualPrinterStatus | null;
  jobs_processed: number;
  config_path: string;
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
  updated_at: string;
  warning: string | null;
  printers: Printer[];
};

// A network the scan can sweep, or the target list a `prepared` event
// reports for the scan it is about to run. Both `discover/networks` and the
// discovery stream's `prepared` event serialize `NetworkResponse` the same
// way, so one type covers both.
export type DiscoveryNetwork = {
  subnet: string;
  interface: string | null;
  hosts: number;
};

// A network adapter the automatic sweep left out, with the shared layer's own
// reason for why (`description`) — the reason only. What to do about it is
// each interface's own wording: the terminal names `--subnet`, while the scan
// options panel points at its custom-network field.
export type SkippedNetwork = {
  interface: string;
  subnet: string | null;
  reason: "too_large" | "unusable_netmask";
  description: string;
};

export type DiscoveryNetworksResponse = {
  networks: DiscoveryNetwork[];
  skipped: SkippedNetwork[];
  default_port: number;
  default_timeout_ms: number;
};

// A printer reported by the discovery stream's `printer` event. Its
// `connection` is the same `UsbConnection | NetworkConnection` shape a
// listed `Printer` carries, since the server serializes both through the
// same `ConnectionResponse`. `interface` is present only for a network
// printer that answered on a known adapter — the server omits the field
// entirely rather than sending `null`.
export type DiscoveredPrinter = {
  transport: "usb" | "network";
  configured_names: string[];
  configured_profile: string | null;
  interface?: string;
  connection: UsbConnection | NetworkConnection;
};

export type AddPrinterBody = {
  name: string;
  profile: string | null;
  connection:
    | { type: "network"; host: string; port: number }
    | {
        type: "usb";
        vendor_id: number;
        product_id: number;
        serial_number: string | null;
        interface_number: number;
        out_endpoint: number;
        in_endpoint: number | null;
      };
};

export type AddPrinterResponse = {
  name: string;
  transport: "usb" | "network";
  profile: string | null;
  warnings: string[];
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

/** The printer state that decides how a text byte reaches the paper. */
export type TextStyle = {
  font: "A" | "B";
  emphasized: boolean;
  underline_thickness: number;
  width_magnification: number;
  height_magnification: number;
  reversed: boolean;
  justification: "left" | "center" | "right";
  code_page: number;
  encoding?: string;
  international_character_set: string;
  right_side_character_spacing_dots: number;
  line_spacing_dots: number;
};

/** The style a printer profile starts a job with. */
export type StyleDefaults = {
  line_spacing_dots: number;
  code_page: number;
  international_character_set: string;
};

export type JobCommand = {
  byte_start: number;
  byte_end: number;
  name: string;
  detail: string;
  /** The bytes that name the command, as uppercase hexadecimal. */
  code_bytes: string;
  /** The first parameter bytes, as uppercase hexadecimal. */
  capped_parameter_bytes: string;
  /** How many parameter bytes the command has in total. */
  total_parameter_bytes: number;
  /** True when the command itself fixes how many parameter bytes follow. */
  fixed_parameters: boolean;
  /** The style after this command, sent only where the command changed it. */
  text_style?: TextStyle;
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
  style_defaults: StyleDefaults;
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
