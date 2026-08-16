import { Route, Router } from "preact-iso";
import { CalibrationPage } from "../features/calibration/page";
import { JobsPage } from "../features/jobs/page";
import { OverviewPage } from "../features/overview/page";
import { PrintersPage } from "../features/printers/page";

function PlaceholderPage({ title }: { title: string }) {
  return (
    <section aria-labelledby={`${title}-heading`} class="space-y-4">
      <h1 id={`${title}-heading`} class="text-3xl font-bold">{title}</h1>
      <p class="text-base-content/70">Loading…</p>
    </section>
  );
}

function ProfilesPage() {
  return <PlaceholderPage title="Profiles" />;
}

function NotFoundPage() {
  return (
    <section aria-labelledby="not-found-heading" class="space-y-4">
      <h1 id="not-found-heading" class="text-3xl font-bold">Not found</h1>
      <p class="text-base-content/70">The requested workbench page does not exist.</p>
    </section>
  );
}

export function AppRoutes() {
  return (
    <Router>
      <Route path="/app/" component={OverviewPage} />
      <Route path="/app/jobs" component={JobsPage} />
      <Route path="/app/printers" component={PrintersPage} />
      <Route path="/app/profiles" component={ProfilesPage} />
      <Route path="/app/calibration" component={CalibrationPage} />
      <Route default component={NotFoundPage} />
    </Router>
  );
}
