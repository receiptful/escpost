import { Route, Router } from "preact-iso";
import { CalibrationPage } from "../features/calibration/page";
import { JobsPage } from "../features/jobs/page";
import { OverviewPage } from "../features/overview/page";
import { PrintersPage } from "../features/printers/page";
import { ProfilesPage } from "../features/profiles/page";

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
      <Route path="/" component={OverviewPage} />
      <Route path="/jobs" component={JobsPage} />
      <Route path="/printers" component={PrintersPage} />
      <Route path="/profiles" component={ProfilesPage} />
      <Route path="/calibration" component={CalibrationPage} />
      <Route default component={NotFoundPage} />
    </Router>
  );
}
