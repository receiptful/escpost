import { LocationProvider } from "preact-iso";
import { AppDataProvider } from "./app/data";
import { PrinterInventoryProvider } from "./app/printer-inventory-data";
import { AppRoutes } from "./app/routes";
import { ServerStatusProvider } from "./app/server-status-data";
import { AppShell } from "./app/shell";

export function App() {
  return (
    <ServerStatusProvider>
      <PrinterInventoryProvider>
        <AppDataProvider>
          <LocationProvider>
            <AppShell>
              <AppRoutes />
            </AppShell>
          </LocationProvider>
        </AppDataProvider>
      </PrinterInventoryProvider>
    </ServerStatusProvider>
  );
}
