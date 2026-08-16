import { createContext } from "preact";
import { useCallback, useContext, useEffect, useRef, useState } from "preact/hooks";
import { NetworkRequestError, getPrinters, getProfiles, getStatus } from "../api/client";
import type { PrintersResponse, ProfilesResponse, StatusResponse } from "../api/types";

type ConnectionState = "loading" | "ready" | "disconnected";
type PrinterPhase = "loading" | "ready" | "refreshing" | "error";

export type PrinterResource = {
  data: PrintersResponse | null;
  error: Error | null;
  phase: PrinterPhase;
};

export type ProfileResource = {
  data: ProfilesResponse | null;
  error: Error | null;
  phase: PrinterPhase;
};

type AppData = {
  connection: ConnectionState;
  status: StatusResponse | null;
  statusError: Error | null;
  printers: PrinterResource;
  refreshPrinters: () => Promise<void>;
  profiles: ProfileResource;
  refreshProfiles: () => Promise<void>;
};

const AppDataContext = createContext<AppData | null>(null);

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

export function AppDataProvider({ children }: { children: preact.ComponentChildren }) {
  const [connection, setConnection] = useState<ConnectionState>("loading");
  const [status, setStatus] = useState<StatusResponse | null>(null);
  const [statusError, setStatusError] = useState<Error | null>(null);
  const [printers, setPrinters] = useState<PrinterResource>(initialPrinters);
  const [profiles, setProfiles] = useState<ProfileResource>(initialProfiles);
  const printerData = useRef<PrintersResponse | null>(null);
  const printerRequest = useRef<Promise<void> | null>(null);
  const printerAbort = useRef<AbortController | null>(null);
  const profileData = useRef<ProfilesResponse | null>(null);
  const profileRequest = useRef<Promise<void> | null>(null);
  const profileAbort = useRef<AbortController | null>(null);

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
    void refreshProfiles();
    poll();
    return () => {
      active = false;
      if (timeout !== undefined) {
        clearTimeout(timeout);
      }
      statusAbort?.abort();
      printerAbort.current?.abort();
      profileAbort.current?.abort();
    };
  }, [refreshPrinters, refreshProfiles]);

  return (
    <AppDataContext.Provider value={{ connection, status, statusError, printers, refreshPrinters, profiles, refreshProfiles }}>
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
