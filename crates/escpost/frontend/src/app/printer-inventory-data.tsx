import { createContext } from "preact";
import { useContext, useEffect, useRef, useState } from "preact/hooks";
import { openPrinterInventoryStream } from "../api/printer-inventory-stream";
import type { PrintersResponse } from "../api/types";

export type PrinterFlashes = Record<string, "found" | "lost">;

export type PrinterInventoryResource =
  | { phase: "checking"; snapshot: null; error: null; printerFlashes: PrinterFlashes }
  | { phase: "ready"; snapshot: PrintersResponse; error: null; printerFlashes: PrinterFlashes }
  | { phase: "disconnected"; snapshot: PrintersResponse | null; error: Error; printerFlashes: PrinterFlashes };

const PrinterInventoryContext = createContext<PrinterInventoryResource | null>(null);
const FLASH_DURATION = 1_200;

function nextFlashes(previous: PrintersResponse | null, next: PrintersResponse, current: PrinterFlashes): PrinterFlashes {
  if (!previous) return current;
  const previousAvailability = new Map(previous.printers.map((printer) => [printer.name, printer.availability]));
  const flashes = { ...current };
  for (const printer of next.printers) {
    const before = previousAvailability.get(printer.name);
    if (before === undefined || (before === "unavailable" && printer.availability === "connected")) {
      flashes[printer.name] = "found";
    } else if (before === "connected" && printer.availability === "unavailable") {
      flashes[printer.name] = "lost";
    }
  }
  return flashes;
}

export function PrinterInventoryProvider({ children }: { children: preact.ComponentChildren }) {
  const [resource, setResource] = useState<PrinterInventoryResource>({
    phase: "checking", snapshot: null, error: null, printerFlashes: {},
  });
  const timeouts = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  useEffect(() => {
    const close = openPrinterInventoryStream({
      onSnapshot: (snapshot) => {
        setResource((current) => {
          const flashes = nextFlashes(current.snapshot, snapshot, current.printerFlashes);
          for (const [name, flash] of Object.entries(flashes)) {
            if (current.printerFlashes[name] === flash) continue;
            const pending = timeouts.current.get(name);
            if (pending !== undefined) clearTimeout(pending);
            timeouts.current.set(name, setTimeout(() => {
              timeouts.current.delete(name);
              setResource((latest) => {
                if (latest.printerFlashes[name] !== flash) return latest;
                const nextFlashes = { ...latest.printerFlashes };
                delete nextFlashes[name];
                return { ...latest, printerFlashes: nextFlashes } as PrinterInventoryResource;
              });
            }, FLASH_DURATION));
          }
          return { phase: "ready", snapshot, error: null, printerFlashes: flashes };
        });
      },
      onError: (error) => {
        setResource((current) => ({
          phase: "disconnected", snapshot: current.snapshot, error, printerFlashes: current.printerFlashes,
        }));
      },
    });
    return () => {
      close();
      for (const timeout of timeouts.current.values()) clearTimeout(timeout);
      timeouts.current.clear();
    };
  }, []);

  return <PrinterInventoryContext.Provider value={resource}>{children}</PrinterInventoryContext.Provider>;
}

export function usePrinterInventory(): PrinterInventoryResource {
  const resource = useContext(PrinterInventoryContext);
  if (!resource) throw new Error("usePrinterInventory must be used within PrinterInventoryProvider.");
  return resource;
}
