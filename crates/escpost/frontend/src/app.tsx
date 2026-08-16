import { LocationProvider } from "preact-iso";
import { AppRoutes } from "./app/routes";
import { AppShell } from "./app/shell";

export function App() {
  return (
    <LocationProvider scope="/app">
      <AppShell>
        <AppRoutes />
      </AppShell>
    </LocationProvider>
  );
}
