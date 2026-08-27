import { createContext } from "preact";
import { useContext, useEffect, useState } from "preact/hooks";
import { openServerStatusStream } from "../api/status-stream";
import type { ServerStatusSnapshot } from "../api/types";

export type ServerStatusResource =
  | { phase: "checking"; snapshot: ServerStatusSnapshot | null; error: null }
  | { phase: "ready"; snapshot: ServerStatusSnapshot; error: null }
  | { phase: "disconnected"; snapshot: ServerStatusSnapshot | null; error: Error };

const ServerStatusContext = createContext<ServerStatusResource | null>(null);

/** How long to wait before opening the status stream again. */
const RETRY_DELAY_MS = 2000;

export function ServerStatusProvider({ children, retryDelayMs = RETRY_DELAY_MS }: {
  children: preact.ComponentChildren;
  retryDelayMs?: number;
}) {
  const [resource, setResource] = useState<ServerStatusResource>({
    phase: "checking",
    snapshot: null,
    error: null,
  });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let retry: ReturnType<typeof setTimeout> | undefined;
    const close = openServerStatusStream({
      onStatus: (snapshot) => {
        setResource({ phase: "ready", snapshot, error: null });
      },
      onError: (error) => {
        setResource((current) => ({ phase: "disconnected", snapshot: current.snapshot, error }));
        // A browser opens a dropped stream again on its own, but gives up for
        // good where the answer is not an event stream, which is what a proxy
        // sends while the server it stands in front of restarts. Opening the
        // stream again is the only way back, thus the status returns on its
        // own rather than waiting for the reader to reload the page.
        retry ??= setTimeout(() => setAttempt((current) => current + 1), retryDelayMs);
      },
    });
    return () => {
      clearTimeout(retry);
      close();
    };
  }, [attempt, retryDelayMs]);

  return <ServerStatusContext.Provider value={resource}>{children}</ServerStatusContext.Provider>;
}

export function useServerStatus(): ServerStatusResource {
  const resource = useContext(ServerStatusContext);
  if (!resource) {
    throw new Error("useServerStatus must be used within ServerStatusProvider.");
  }
  return resource;
}
