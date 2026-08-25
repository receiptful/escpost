import { useEffect, useRef } from "preact/hooks";
import { useAppData } from "./data";
import { useServerStatus } from "./server-status-data";

export function ReconnectPrinters() {
  const { refreshPrinters } = useAppData();
  const { phase } = useServerStatus();
  const previousPhase = useRef(phase);

  useEffect(() => {
    if (previousPhase.current === "disconnected" && phase === "ready") {
      void refreshPrinters({ force: true });
    }
    previousPhase.current = phase;
  }, [phase, refreshPrinters]);

  return null;
}
