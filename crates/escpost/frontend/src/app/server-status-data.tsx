import { createContext } from "preact";
import { useContext, useEffect, useState } from "preact/hooks";
import { openServerStatusStream } from "../api/status-stream";
import type { ServerStatusSnapshot } from "../api/types";

export type ServerStatusResource =
  | { phase: "checking"; snapshot: ServerStatusSnapshot | null; error: null }
  | { phase: "ready"; snapshot: ServerStatusSnapshot; error: null }
  | { phase: "disconnected"; snapshot: ServerStatusSnapshot | null; error: Error };

const ServerStatusContext = createContext<ServerStatusResource | null>(null);

export function ServerStatusProvider({ children }: { children: preact.ComponentChildren }) {
  const [resource, setResource] = useState<ServerStatusResource>({
    phase: "checking",
    snapshot: null,
    error: null,
  });

  useEffect(() => openServerStatusStream({
    onStatus: (snapshot) => {
      setResource({ phase: "ready", snapshot, error: null });
    },
    onError: (error) => {
      setResource((current) => ({ phase: "disconnected", snapshot: current.snapshot, error }));
    },
  }), []);

  return <ServerStatusContext.Provider value={resource}>{children}</ServerStatusContext.Provider>;
}

export function useServerStatus(): ServerStatusResource {
  const resource = useContext(ServerStatusContext);
  if (!resource) {
    throw new Error("useServerStatus must be used within ServerStatusProvider.");
  }
  return resource;
}
