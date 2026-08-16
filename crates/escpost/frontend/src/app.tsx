import { LocationProvider } from "preact-iso";
import { AppDataProvider } from "./app/data";
import { AppRoutes } from "./app/routes";
import { AppShell } from "./app/shell";

export function App() {
  return (
    <AppDataProvider>
      <LocationProvider scope="/app">
        <AppShell>
          <AppRoutes />
        </AppShell>
      </LocationProvider>
    </AppDataProvider>
  );
}
