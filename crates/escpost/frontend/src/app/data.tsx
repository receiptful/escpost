import { createContext } from "preact";
import { useCallback, useContext, useEffect, useRef, useState } from "preact/hooks";
import { getProfiles } from "../api/client";
import { openDiscoveryStream } from "../api/discovery-stream";
import type { DiscoveryQuery, UsbDiscoveryFailure } from "../api/discovery-stream";
import type { AddPrinterBody, DiscoveredPrinter, ProfilesResponse } from "../api/types";

type ResourcePhase = "loading" | "ready" | "refreshing" | "error";
type ScanPhase = "idle" | "running" | "done" | "stopped" | "error";

export type ScanState = {
  phase: ScanPhase;
  completed: number;
  total: number;
  printers: DiscoveredPrinter[];
  failures: UsbDiscoveryFailure[];
  error: string | null;
};

export type ProfileResource = { data: ProfilesResponse | null; error: Error | null; phase: ResourcePhase };

type AppData = {
  profiles: ProfileResource;
  ensureProfiles: () => Promise<void>;
  refreshProfiles: () => Promise<void>;
  scan: ScanState;
  scanQuery: DiscoveryQuery;
  startScan: (query: DiscoveryQuery) => void;
  cancelScan: () => void;
  markScanResultConfigured: (name: string, connection: AddPrinterBody["connection"]) => void;
};

const AppDataContext = createContext<AppData | null>(null);
const initialProfiles: ProfileResource = { data: null, error: null, phase: "loading" };
const initialScanQuery: DiscoveryQuery = { usb: true, network: true, subnets: [] };
const initialScan: ScanState = { phase: "idle", completed: 0, total: 0, printers: [], failures: [], error: null };

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
  const [profiles, setProfiles] = useState<ProfileResource>(initialProfiles);
  const [scan, setScan] = useState<ScanState>(initialScan);
  const [scanQuery, setScanQuery] = useState<DiscoveryQuery>(initialScanQuery);
  const profileData = useRef<ProfilesResponse | null>(null);
  const profileRequest = useRef<Promise<void> | null>(null);
  const profileAbort = useRef<AbortController | null>(null);
  const scanCloser = useRef<(() => void) | null>(null);

  const closeScan = useCallback(() => {
    scanCloser.current?.();
    scanCloser.current = null;
  }, []);

  const handleDiscoveredPrinter = useCallback((printer: DiscoveredPrinter) => {
    setScan((current) => ({ ...current, printers: [...current.printers, printer] }));
  }, []);

  const startScan = useCallback((query: DiscoveryQuery) => {
    closeScan();
    setScan({ ...initialScan, phase: "running" });
    setScanQuery(query);
    scanCloser.current = openDiscoveryStream(query, {
      onPrepared: (prepared) => setScan((current) => ({ ...current, total: prepared.total_probes })),
      onPrinter: handleDiscoveredPrinter,
      onProgress: (progress) => setScan((current) => ({ ...current, completed: progress.completed, total: progress.total })),
      onUsbFailure: (failure) => setScan((current) => ({ ...current, failures: [...current.failures, failure] })),
      onCompleted: () => { scanCloser.current = null; setScan((current) => ({ ...current, phase: "done" })); },
      onError: (error) => { scanCloser.current = null; setScan((current) => ({ ...current, phase: "error", error })); },
    });
  }, [closeScan, handleDiscoveredPrinter]);

  const cancelScan = useCallback(() => {
    closeScan();
    setScan((current) => current.phase === "running" ? { ...current, phase: "stopped" } : current);
  }, [closeScan]);

  const markScanResultConfigured = useCallback((name: string, connection: AddPrinterBody["connection"]) => {
    setScan((current) => {
      const index = current.printers.findIndex((printer) => registeredAs(printer, connection) && !printer.configured_names.includes(name));
      if (index === -1) return current;
      const printers = [...current.printers];
      const found = printers[index]!;
      printers[index] = { ...found, configured_names: [...found.configured_names, name] };
      return { ...current, printers };
    });
  }, []);

  const refreshProfiles = useCallback(async () => {
    if (profileRequest.current) return profileRequest.current;
    const controller = new AbortController();
    profileAbort.current = controller;
    setProfiles((current) => ({ data: current.data, error: null, phase: current.data ? "refreshing" : "loading" }));
    const request = getProfiles(controller.signal)
      .then((data) => { profileData.current = data; setProfiles({ data, error: null, phase: "ready" }); })
      .catch((error: unknown) => {
        if (controller.signal.aborted) return;
        setProfiles({ data: profileData.current, error: error instanceof Error ? error : new Error("Unable to load profile catalog."), phase: profileData.current ? "ready" : "error" });
      })
      .finally(() => { if (profileAbort.current === controller) profileAbort.current = null; profileRequest.current = null; });
    profileRequest.current = request;
    return request;
  }, []);

  const ensureProfiles = useCallback(async () => {
    if (!profileData.current) return refreshProfiles();
  }, [refreshProfiles]);

  useEffect(() => () => { closeScan(); profileAbort.current?.abort(); }, [closeScan]);

  return <AppDataContext.Provider value={{ profiles, ensureProfiles, refreshProfiles, scan, scanQuery, startScan, cancelScan, markScanResultConfigured }}>{children}</AppDataContext.Provider>;
}

export function useAppData() {
  const data = useContext(AppDataContext);
  if (!data) throw new Error("useAppData must be used within AppDataProvider.");
  return data;
}
