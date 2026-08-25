import { createContext } from "preact";
import { useCallback, useContext, useEffect, useRef, useState } from "preact/hooks";
import { getPrinters, getProfiles } from "../api/client";
import { openDiscoveryStream } from "../api/discovery-stream";
import type { DiscoveryQuery, UsbDiscoveryFailure } from "../api/discovery-stream";
import type { AddPrinterBody, DiscoveredPrinter, PrintersResponse, ProfilesResponse } from "../api/types";

type ResourcePhase = "loading" | "ready" | "refreshing" | "error";
// `stopped` is its own phase rather than a flag on `done`, because it is a
// different fact about the same results: a scan that was interrupted knows
// how far it got, and every reader of this state has to be able to tell that
// from one that ran out of addresses.
type ScanPhase = "idle" | "running" | "done" | "stopped" | "error";

// Owns the discovery scan across page navigation: a scan started from the
// printers page keeps running (and this state keeps updating) even after the
// user leaves it, because it lives here rather than in a route component.
export type ScanState = {
  phase: ScanPhase;
  completed: number;
  total: number;
  printers: DiscoveredPrinter[];
  failures: UsbDiscoveryFailure[];
  error: string | null;
};

// A printer that transitioned availability since the previous printers
// response, or that a scan just proved reachable. Each entry is transient:
// consumers clear it after showing a brief flash.
export type PrinterFlashes = Record<string, "found" | "lost">;

export type PrinterResource = {
  data: PrintersResponse | null;
  error: Error | null;
  phase: ResourcePhase;
};

export type ProfileResource = {
  data: ProfilesResponse | null;
  error: Error | null;
  phase: ResourcePhase;
};

type AppData = {
  printers: PrinterResource;
  // `force` queues a fresh request behind one already in flight instead of
  // joining it. A caller that has just *changed* the inventory needs an
  // answer that was asked for after the change; the in-flight one was asked
  // for before it and cannot contain it. The returned promise settles when
  // that fresh request does, so awaiting a forced refresh means the inventory
  // has caught up — an unforced one may settle on a response that predates
  // anything the caller did.
  refreshPrinters: (options?: { force?: boolean }) => Promise<void>;
  profiles: ProfileResource;
  ensureProfiles: () => Promise<void>;
  refreshProfiles: () => Promise<void>;
  scan: ScanState;
  // The scope the running or most recent scan was started with, and what a
  // repeat of it would send. It lives here rather than in the page for the
  // same reason the scan does: navigation unmounts the page, and a sweep
  // narrowed to one segment must not silently widen back to every network
  // this machine is on.
  scanQuery: DiscoveryQuery;
  startScan: (query: DiscoveryQuery) => void;
  cancelScan: () => void;
  printerFlashes: PrinterFlashes;
  flashPrinter: (name: string, kind: "found" | "lost") => void;
  markScanResultConfigured: (name: string, connection: AddPrinterBody["connection"]) => void;
};

const AppDataContext = createContext<AppData | null>(null);
const PRINTER_POLL_INTERVAL = 10_000;
const FLASH_DURATION = 1_200;

const initialPrinters: PrinterResource = {
  data: null,
  error: null,
  phase: "loading",
};

const initialProfiles: ProfileResource = {
  data: null,
  error: null,
  phase: "loading",
};

// What a scan nobody has configured runs with: the CLI's own no-flag
// behaviour, both transports, targets detected automatically. It names no
// port and no timeout because nobody has chosen either, and the endpoint owns
// both defaults — a number restated here would be invisible in the interface
// and would silently outlive the server's own.
const initialScanQuery: DiscoveryQuery = { usb: true, network: true, subnets: [] };

const initialScan: ScanState = {
  phase: "idle",
  completed: 0,
  total: 0,
  printers: [],
  failures: [],
  error: null,
};

// Whether a discovered printer is the device that was just registered, by the
// shared layer's own rules. `discover::configured_names` pairs a network
// result with a configured host and port; `inventory::configuration_matches`
// pairs a USB device with the vendor, product and interface its route was
// saved under, requires the saved OUT endpoint to be one the device exposes,
// and treats a configured printer with no serial number as matching any
// serial. Getting this wrong in either direction only ever changes which
// results the panel offers — the server remains the authority on a
// collision — but it is worth mirroring exactly rather than approximating.
function registeredAs(discovered: DiscoveredPrinter, connection: AddPrinterBody["connection"]) {
  const found = discovered.connection;
  if (connection.type === "network") {
    return found.type === "network" && found.host === connection.host && found.port === connection.port;
  }
  return found.type === "usb"
    && found.vendor_id === connection.vendor_id
    && found.product_id === connection.product_id
    && found.interface_number === connection.interface_number
    && found.out_endpoints.includes(connection.out_endpoint)
    && (connection.serial_number === null || found.serial_number === connection.serial_number);
}

export function AppDataProvider({ children }: { children: preact.ComponentChildren }) {
  const [printers, setPrinters] = useState<PrinterResource>(initialPrinters);
  const [profiles, setProfiles] = useState<ProfileResource>(initialProfiles);
  const [scan, setScan] = useState<ScanState>(initialScan);
  const [scanQuery, setScanQuery] = useState<DiscoveryQuery>(initialScanQuery);
  const [printerFlashes, setPrinterFlashes] = useState<PrinterFlashes>({});
  const printerData = useRef<PrintersResponse | null>(null);
  const printerRequest = useRef<Promise<void> | null>(null);
  const printerAbort = useRef<AbortController | null>(null);
  // The follow-up request a forced refresh asked for while another was in
  // flight, and the handle that settles the promise its caller is holding.
  // One entry however many forced calls arrive: they all want the same thing,
  // which is one inventory fetched after their change.
  const printerRefreshPending = useRef<{ promise: Promise<void>; settle: () => void } | null>(null);
  const profileData = useRef<ProfilesResponse | null>(null);
  const profileRequest = useRef<Promise<void> | null>(null);
  const profileAbort = useRef<AbortController | null>(null);
  const scanCloser = useRef<(() => void) | null>(null);
  const flashTimeouts = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  // Flags `name` as newly "found" or "lost" and clears it again after a
  // short window, so a consumer rendering `printerFlashes` sees a single
  // pulse per transition rather than a state that lingers. A second flash of
  // the same kind for the same name restarts the window instead of stacking
  // timeouts.
  const flashPrinter = useCallback((name: string, kind: "found" | "lost") => {
    setPrinterFlashes((current) => ({ ...current, [name]: kind }));
    const pending = flashTimeouts.current.get(name);
    if (pending !== undefined) {
      clearTimeout(pending);
    }
    const timeout = setTimeout(() => {
      flashTimeouts.current.delete(name);
      setPrinterFlashes((current) => {
        if (current[name] !== kind) {
          return current;
        }
        const next = { ...current };
        delete next[name];
        return next;
      });
    }, FLASH_DURATION);
    flashTimeouts.current.set(name, timeout);
  }, []);

  // Compares a freshly fetched printers response against the previously
  // cached one and flashes only the printers whose availability actually
  // changed, so a poll that repeats the same state is silent.
  const diffAvailability = useCallback((previous: PrintersResponse | null, next: PrintersResponse) => {
    if (!previous) {
      return;
    }
    const previousAvailability = new Map(previous.printers.map((printer) => [printer.name, printer.availability]));
    for (const printer of next.printers) {
      const before = previousAvailability.get(printer.name);
      if (before === "connected" && printer.availability === "unavailable") {
        flashPrinter(printer.name, "lost");
      } else if (before === "unavailable" && printer.availability === "connected") {
        flashPrinter(printer.name, "found");
      }
    }
  }, [flashPrinter]);

  // A `printer` event whose `configured_names` is non-empty means the scan
  // just proved that printer reachable directly, rather than through the
  // next poll. Flash those names and fold the evidence into the cached
  // printers response immediately instead of waiting up to
  // `PRINTER_POLL_INTERVAL` for the poll to catch up.
  const handleDiscoveredPrinter = useCallback((printer: DiscoveredPrinter) => {
    setScan((current) => ({ ...current, printers: [...current.printers, printer] }));
    if (printer.configured_names.length === 0) {
      return;
    }
    for (const name of printer.configured_names) {
      flashPrinter(name, "found");
    }
    const cached = printerData.current;
    if (!cached) {
      return;
    }
    const names = new Set(printer.configured_names);
    const data: PrintersResponse = {
      printers: cached.printers.map((entry) => (names.has(entry.name) ? { ...entry, availability: "connected" as const } : entry)),
    };
    printerData.current = data;
    setPrinters((state) => ({ ...state, data }));
  }, [flashPrinter]);

  // Closes whatever scan stream is currently open, if any. Used both by
  // `cancelScan` and by `startScan` itself: a rescan must close the previous
  // stream before opening a new one, or the old scan keeps running on the
  // server.
  const closeScan = useCallback(() => {
    if (scanCloser.current) {
      scanCloser.current();
      scanCloser.current = null;
    }
  }, []);

  const startScan = useCallback((query: DiscoveryQuery) => {
    closeScan();
    setScan({ ...initialScan, phase: "running" });
    // Remembered rather than merely used, so that repeating this scan repeats
    // what was actually asked for. A cancel leaves it alone: stopping a sweep
    // is not a change of mind about its scope, and `printers discover` does
    // not forget your flags when you interrupt it either.
    setScanQuery(query);
    scanCloser.current = openDiscoveryStream(query, {
      onPrepared: (prepared) => {
        setScan((current) => ({ ...current, total: prepared.total_probes }));
      },
      onPrinter: handleDiscoveredPrinter,
      onProgress: (progress) => {
        setScan((current) => ({ ...current, completed: progress.completed, total: progress.total }));
      },
      onUsbFailure: (failure) => {
        setScan((current) => ({ ...current, failures: [...current.failures, failure] }));
      },
      onCompleted: () => {
        scanCloser.current = null;
        setScan((current) => ({ ...current, phase: "done" }));
      },
      onError: (message) => {
        scanCloser.current = null;
        setScan((current) => ({ ...current, phase: "error", error: message }));
      },
    });
  }, [closeScan, handleDiscoveredPrinter]);

  // Stopping the probing, not forgetting what it found. Closing the stream is
  // still the whole of cancellation on the wire; what changes is that the
  // printers and failures already in hand stay in hand, because a sweep that
  // reached a printer before it was interrupted has produced something worth
  // keeping.
  const cancelScan = useCallback(() => {
    closeScan();
    setScan((current) => current.phase === "running" ? { ...current, phase: "stopped" } : current);
  }, [closeScan]);

  // Records that a scan result has just been registered under `name`. The
  // stream computed `configured_names` from the configuration as it stood
  // when the scan began, so a printer added since then would still be offered
  // as new; the panel hides configured hits and counts them, so stating that
  // it is now configured is all it takes for the row to move.
  //
  // This lives beside the scan rather than in the page because the scan does:
  // a route change unmounts the page, and a printer registered before that
  // must not be offered again after it.
  // One result, not every result that matches: `classify_usb_printers` gives
  // a saved printer at most one connected interface, so the terminal keeps
  // offering the second of two devices that share a vendor, product and
  // absent serial after the first is registered. Marking both would hide a
  // printer nobody registered until a rescan corrected it. For a network
  // result the distinction is theoretical — one host and port is one result —
  // but the rule is the shared layer's, so it is followed rather than
  // reasoned around.
  const markScanResultConfigured = useCallback((name: string, connection: AddPrinterBody["connection"]) => {
    setScan((current) => {
      const index = current.printers.findIndex((printer) => (
        registeredAs(printer, connection) && !printer.configured_names.includes(name)
      ));
      if (index === -1) {
        return current;
      }
      const printers = [...current.printers];
      const found = printers[index]!;
      printers[index] = { ...found, configured_names: [...found.configured_names, name] };
      return { ...current, printers };
    });
  }, []);

  useEffect(() => {
    return () => {
      closeScan();
      for (const timeout of flashTimeouts.current.values()) {
        clearTimeout(timeout);
      }
      flashTimeouts.current.clear();
    };
  }, [closeScan]);

  const refreshPrinters = useCallback(async (options?: { force?: boolean }) => {
    if (printerRequest.current) {
      // A forced refresh cannot be satisfied by a request that was issued
      // before the change it is asking about — the response is already on its
      // way and cannot contain it — so a fresh one is queued behind it
      // instead. Deduping still applies to ordinary polls.
      if (!options?.force) {
        return printerRequest.current;
      }
      if (!printerRefreshPending.current) {
        let settle!: () => void;
        const promise = new Promise<void>((resolve) => { settle = resolve; });
        printerRefreshPending.current = { promise, settle };
      }
      // The caller's promise is the follow-up's, never the stale request's:
      // awaiting a forced refresh has to mean the inventory has caught up,
      // which is exactly the mistake this branch exists to prevent. It stays
      // unsettled if the provider unmounts before the follow-up runs, which
      // is the same nothing every caller already does with it.
      return printerRefreshPending.current.promise;
    }

    const controller = new AbortController();
    printerAbort.current = controller;
    setPrinters((current) => ({
      data: current.data,
      error: null,
      phase: current.data ? "refreshing" : "loading",
    }));
    const request = getPrinters(undefined, controller.signal)
      .then((data) => {
        diffAvailability(printerData.current, data);
        printerData.current = data;
        setPrinters({ data, error: null, phase: "ready" });
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) {
          return;
        }
        setPrinters({
          data: printerData.current,
          error: error instanceof Error ? error : new Error("Unable to load printer inventory."),
          phase: printerData.current ? "ready" : "error",
        });
      })
      .finally(() => {
        if (printerAbort.current === controller) {
          printerAbort.current = null;
        }
        printerRequest.current = null;
        const pending = printerRefreshPending.current;
        if (pending) {
          printerRefreshPending.current = null;
          // Issued here, inside this request's own settle, so it goes out
          // before the poll loop re-arms rather than racing it.
          void refreshPrinters().finally(pending.settle);
        }
      });
    printerRequest.current = request;
    return request;
  }, [diffAvailability]);

  const refreshProfiles = useCallback(async () => {
    if (profileRequest.current) {
      return profileRequest.current;
    }

    const controller = new AbortController();
    profileAbort.current = controller;
    setProfiles((current) => ({
      data: current.data,
      error: null,
      phase: current.data ? "refreshing" : "loading",
    }));
    const request = getProfiles(controller.signal)
      .then((data) => {
        profileData.current = data;
        setProfiles({ data, error: null, phase: "ready" });
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) {
          return;
        }
        setProfiles({
          data: profileData.current,
          error: error instanceof Error ? error : new Error("Unable to load profile catalog."),
          phase: profileData.current ? "ready" : "error",
        });
      })
      .finally(() => {
        if (profileAbort.current === controller) {
          profileAbort.current = null;
        }
        profileRequest.current = null;
      });
    profileRequest.current = request;
    return request;
  }, []);

  const ensureProfiles = useCallback(async () => {
    if (profileData.current) {
      return;
    }
    return refreshProfiles();
  }, [refreshProfiles]);

  useEffect(() => {
    let active = true;
    let printerTimeout: ReturnType<typeof setTimeout> | undefined;

    // A poll against a dead printer can take several seconds now that the
    // backend retries before confirming it unreachable, so the settle-then-
    // rearm shape matters: a `setInterval` could stack a second request on
    // top of one still in flight. Re-arming is also gated on the tab being
    // visible, so a background tab stops polling instead of doing pointless
    // work no one can see.
    const pollPrinters = () => {
      void refreshPrinters().finally(() => {
        if (active && document.visibilityState === "visible") {
          printerTimeout = setTimeout(pollPrinters, PRINTER_POLL_INTERVAL);
        }
      });
    };

    const handleVisibilityChange = () => {
      if (document.visibilityState === "hidden") {
        if (printerTimeout !== undefined) {
          clearTimeout(printerTimeout);
          printerTimeout = undefined;
        }
        return;
      }
      if (printerTimeout !== undefined) {
        clearTimeout(printerTimeout);
        printerTimeout = undefined;
      }
      // Only kick off an immediate poll if nothing is already in flight. A
      // request already in flight will re-arm on its own once it settles,
      // since visibility is checked at settle time — starting a second one
      // here would just be deduped by `refreshPrinters`, but would still
      // leave two `finally` callbacks racing to schedule the next timeout.
      if (!printerRequest.current) {
        pollPrinters();
      }
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);
    pollPrinters();
    return () => {
      active = false;
      if (printerTimeout !== undefined) {
        clearTimeout(printerTimeout);
      }
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      printerAbort.current?.abort();
      printerRefreshPending.current = null;
      profileAbort.current?.abort();
    };
  }, [refreshPrinters]);

  return (
    <AppDataContext.Provider
      value={{
        printers,
        refreshPrinters,
        profiles,
        ensureProfiles,
        refreshProfiles,
        scan,
        scanQuery,
        startScan,
        cancelScan,
        printerFlashes,
        // Exposed because a *newly added* printer has no availability
        // transition to diff against: it is simply absent from the previous
        // inventory and present in the next one. Registering it is the third
        // event the flash treatment marks, and only the caller that
        // registered it knows it happened.
        flashPrinter,
        markScanResultConfigured,
      }}
    >
      {children}
    </AppDataContext.Provider>
  );
}

export function useAppData() {
  const data = useContext(AppDataContext);
  if (!data) {
    throw new Error("useAppData must be used within AppDataProvider.");
  }
  return data;
}
