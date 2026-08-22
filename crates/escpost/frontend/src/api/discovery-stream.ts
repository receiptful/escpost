import type { DiscoveredPrinter, DiscoveryNetwork, SkippedNetwork } from "./types";

// Mirrors the CLI's own flags: no filter when both transports are selected
// (matching the CLI's no-flag behavior), a repeated `subnet` per network,
// `port`, and `timeout`. Nothing else is sent.
//
// `port` and `timeoutMs` are optional because an unchosen one is not a value
// this client owns: omitting the parameter is how the CLI omits a flag, and
// the endpoint applies the very same defaults it advertises on
// `discover/networks`. Restating a number the server owns would go stale
// silently and scan the wrong port. They are carried even for a USB-only
// scan, where the card's fields are disabled rather than cleared, and
// dropped on the wire — see `discoveryQueryString`.
export type DiscoveryQuery = {
  usb: boolean;
  network: boolean;
  subnets: string[];
  port?: number;
  timeoutMs?: number;
};

// Payload of the stream's `prepared` event: the scan targets and skipped
// adapters the scope resolved to, before the first probe goes out.
export type DiscoveryPrepared = {
  targets: DiscoveryNetwork[];
  skipped: SkippedNetwork[];
  total_probes: number;
};

export type DiscoveryProgress = {
  completed: number;
  total: number;
};

// One tolerated USB enumeration failure. `can_grant_usb_permissions` is a
// fact about the server's platform, not a remedy: `printers
// grant-usb-permissions` is a Linux-only subcommand, and only the server
// knows what it runs on. The browser still words its own remedy, and stays
// silent about a command the host would not recognize.
export type UsbDiscoveryFailure = {
  vendor_id: number;
  product_id: number;
  stage: "open_device" | "inspect_configuration";
  reason: string;
  permission_denied: boolean;
  can_grant_usb_permissions: boolean;
};

export type DiscoveryHandlers = {
  onPrepared: (event: DiscoveryPrepared) => void;
  onPrinter: (printer: DiscoveredPrinter) => void;
  onProgress: (progress: DiscoveryProgress) => void;
  onUsbFailure: (failure: UsbDiscoveryFailure) => void;
  onCompleted: () => void;
  onError: (message: string) => void;
};

export function discoveryQueryString(query: DiscoveryQuery) {
  const parameters = new URLSearchParams();
  if (query.usb !== query.network) {
    parameters.set("transport", query.usb ? "usb" : "network");
  }
  // A USB-only scan takes no network options at all. `printers discover
  // --transport usb` refuses `--subnet`, `--port` and `--timeout` outright
  // (`CliError::NetworkScanOptionForUsbDiscovery`), and the endpoint builds
  // the very same arguments, so sending the defaults here is not a harmless
  // restatement — it is a 400.
  if (!query.network) {
    return parameters.toString();
  }
  for (const subnet of query.subnets) {
    parameters.append("subnet", subnet);
  }
  if (query.port !== undefined) {
    parameters.set("port", String(query.port));
  }
  if (query.timeoutMs !== undefined) {
    parameters.set("timeout", String(query.timeoutMs));
  }
  return parameters.toString();
}

function listen<T>(source: EventSource, name: string, handle: (payload: T) => void) {
  source.addEventListener(name, (event) => {
    handle(JSON.parse((event as MessageEvent).data) as T);
  });
}

// The server's `error` event carries `{ "message": string }`; a genuine
// connection failure (the browser's own generic `error` event, dispatched
// when the stream drops without one of these) carries no such payload. Both
// arrive through the same `EventSource` "error" listener, so this extracts
// the server's wording when there is one and falls back to a generic message
// otherwise.
function errorMessage(event: Event): string {
  const data = (event as MessageEvent).data as string | undefined;
  if (typeof data === "string") {
    try {
      const payload = JSON.parse(data) as { message?: unknown };
      if (typeof payload.message === "string") {
        return payload.message;
      }
    } catch {
      // Fall through to the generic message below.
    }
  }
  return "The discovery stream ended unexpectedly.";
}

/**
 * Opens the discovery stream and returns the function that cancels the scan.
 *
 * Cancellation is not an endpoint: closing the `EventSource` drops the HTTP
 * response, which drops the scan future on the server, which aborts every
 * outstanding probe. The returned closer is therefore the only cancellation
 * mechanism, and it is also invoked internally on `completed` and on `error`
 * — `EventSource` reconnects automatically by default, and a stream that
 * ended normally would otherwise be reopened by the browser, silently
 * starting a second scan.
 */
export function openDiscoveryStream(query: DiscoveryQuery, handlers: DiscoveryHandlers): () => void {
  // A scan that chose nothing at all sends no query string rather than a bare
  // `?`, which is what `printers discover` with no flags is.
  const parameters = discoveryQueryString(query);
  const source = new EventSource(parameters === "" ? "/api/printers/discover" : `/api/printers/discover?${parameters}`);

  listen<DiscoveryPrepared>(source, "prepared", handlers.onPrepared);
  listen<DiscoveredPrinter>(source, "printer", handlers.onPrinter);
  listen<DiscoveryProgress>(source, "progress", handlers.onProgress);
  listen<UsbDiscoveryFailure>(source, "usb_failure", handlers.onUsbFailure);

  source.addEventListener("completed", () => {
    source.close();
    handlers.onCompleted();
  });
  source.addEventListener("error", (event) => {
    source.close();
    handlers.onError(errorMessage(event));
  });

  return () => source.close();
}
