import { createContext } from "preact";
import { useCallback, useContext, useEffect, useRef, useState } from "preact/hooks";
import { NetworkRequestError, getPrinters, getStatus } from "../api/client";
import type { PrintersResponse, StatusResponse } from "../api/types";

type ConnectionState = "loading" | "ready" | "disconnected";
type PrinterPhase = "loading" | "ready" | "refreshing" | "error";

export type PrinterResource = {
  data: PrintersResponse | null;
  error: Error | null;
  phase: PrinterPhase;
};

type AppData = {
  connection: ConnectionState;
  status: StatusResponse | null;
  statusError: Error | null;
  printers: PrinterResource;
  refreshPrinters: () => Promise<void>;
};

const AppDataContext = createContext<AppData | null>(null);

const initialPrinters: PrinterResource = {
  data: null,
  error: null,
  phase: "loading",
};

export function AppDataProvider({ children }: { children: preact.ComponentChildren }) {
  const [connection, setConnection] = useState<ConnectionState>("loading");
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [statusError, setStatusError] = useState<Error | null>(null);
  const [printers, setPrinters] = useState<PrinterResource>(initialPrinters);
  const printerData = useRef<PrintersResponse | null>(null);
  const printerRequest = useRef<Promise<void> | null>(null);
  const printerAbort = useRef<AbortController | null>(null);

  const refreshPrinters = useCallback(async () => {
    if (printerRequest.current) {
      return printerRequest.current;
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
      });
    printerRequest.current = request;
    return request;
  }, []);

  useEffect(() => {
    let active = true;
    let timeout: ReturnType<typeof setTimeout> | undefined;
    let statusAbort: AbortController | null = null;
    let disconnected = false;

    const poll = () => {
      statusAbort = new AbortController();
      void getStatus(statusAbort.signal)
        .then((nextStatus) => {
          if (!active) {
            return;
          }
          setStatus(nextStatus);
          setStatusError(null);
          setConnection("ready");
          if (disconnected) {
            disconnected = false;
            void refreshPrinters();
          }
        })
        .catch((error: unknown) => {
          if (!active || statusAbort?.signal.aborted) {
            return;
          }
          const reported = error instanceof Error ? error : new Error("Status is unavailable.");
          setStatusError(reported);
          if (error instanceof NetworkRequestError) {
            disconnected = true;
            setConnection("disconnected");
          }
        })
        .finally(() => {
          if (active) {
            timeout = setTimeout(poll, 2_000);
          }
        });
    };

    void refreshPrinters();
    poll();
    return () => {
      active = false;
      if (timeout !== undefined) {
        clearTimeout(timeout);
      }
      statusAbort?.abort();
      printerAbort.current?.abort();
    };
  }, [refreshPrinters]);

  return (
    <AppDataContext.Provider value={{ connection, status, statusError, printers, refreshPrinters }}>
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
