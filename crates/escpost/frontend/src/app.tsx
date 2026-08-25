import { LocationProvider } from "preact-iso";
import { AppDataProvider } from "./app/data";
import { ReconnectPrinters } from "./app/reconnect-printers";
import { AppRoutes } from "./app/routes";
import { ServerStatusProvider } from "./app/server-status-data";
import { AppShell } from "./app/shell";

export function App() {
  return (
    <ServerStatusProvider>
      <AppDataProvider>
        <ReconnectPrinters />
        <LocationProvider scope="/app">
          <AppShell>
            <AppRoutes />
          </AppShell>
        </LocationProvider>
      </AppDataProvider>
    </ServerStatusProvider>
  );
}
