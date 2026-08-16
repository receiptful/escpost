export function JobsPage() {
  return (
    <section aria-labelledby="jobs-heading" class="space-y-4">
      <div>
        <p class="text-sm font-semibold text-primary">Workbench</p>
        <h1 id="jobs-heading" class="mt-1 text-3xl font-bold">Print jobs</h1>
      </div>
      <p class="max-w-2xl text-base-content/70">
        The current job viewer remains available while the workbench job history is prepared.
      </p>
      <a class="btn btn-primary" href="/">
        Open current job viewer
      </a>
    </section>
  );
}
