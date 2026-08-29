# AGENTS

Guidance for automated agents and contributors working in this repository.

## Everything runs through Docker Compose

Do **not** invoke `cargo`, `rustc`, or the Python toolchain on the host — they
are not installed there. All builds, tests, linting, and the CLI run inside the
`escpost-development` image defined in `compose.yaml`. The container mounts the
working tree at `/workspace` and caches `cargo` state in named volumes, so
incremental builds stay fast across invocations.

Build the image once (and after changing the `Dockerfile`):

```bash
docker compose build
```

### Warm a new worktree from the main checkout

Docker Compose gives each worktree its own named volumes. Before the first
build in a new worktree, seed those volumes from the main checkout to reuse
downloaded dependencies, compiled artifacts, and the developer's saved printer
configuration. Run this from the new worktree while none of its Compose
services are running:

```bash
set -eu

main_compose_project=escpost
worktree_compose_project=$(basename "$PWD")

# Create the destination volumes without starting the services.
docker compose create

for volume in \
  cargo-home \
  cargo-target \
  python-venv \
  frontend-node-modules \
  escpost-config
do
  # Inspect first: `docker run -v` would silently create a misspelled volume.
  docker volume inspect "${main_compose_project}_${volume}" >/dev/null
  docker volume inspect "${worktree_compose_project}_${volume}" >/dev/null

  docker run --rm \
    -v "${main_compose_project}_${volume}:/source:ro" \
    -v "${worktree_compose_project}_${volume}:/destination" \
    escpost-development \
    sh -c 'cp -a /source/. /destination/'
done
```

The default project name is the checkout directory name. Adjust
`main_compose_project` or `worktree_compose_project` if Compose was invoked
with `--project-name` or `COMPOSE_PROJECT_NAME`; `docker volume ls` shows the
actual names. If the worktree has already run builds, stop its containers
before copying rather than modifying a volume that Cargo or Bun is using.

### Common commands

```bash
# Whole-workspace test run
docker compose run --rm test cargo test --workspace

# A single crate
docker compose run --rm test cargo test -p escpost-render

# Formatting and lints (must pass before committing)
docker compose run --rm test cargo fmt --check
docker compose run --rm test cargo clippy --workspace --all-targets -- -D warnings

# Golden conformance layer, with rendered-vs-expected detail
docker compose run --rm test cargo test -p escpost-render --test golden_cases -- --nocapture

# Frontend tests, type checking, and production bundle
docker compose run --rm frontend bun test
docker compose run --rm frontend bun run typecheck
docker compose run --rm frontend-build

# Axum with restart-on-change plus Vite hot reload
docker compose up
```

### The web app

Open `http://127.0.0.1:5173/` during development. Vite serves the web app
from `crates/escpost/frontend/src/` with hot reload, and forwards each `/api`
request to the backend on port 9000, thus the web app calls the API on its
own origin.

The backend on port 9000 serves the API. It does not serve the web app during
development; that comes from Vite.

A release build embeds `crates/escpost/frontend/dist/` in the binary. The
`test` service builds that bundle, because the Rust tests request the web app from
the server. Do not commit `dist/` or `node_modules/`; commit `bun.lock`.

### Running the CLI

The `escpost` service builds and runs the compiled binary, maps USB devices,
and persists printer configuration in a Compose-managed named volume:

```bash
docker compose run --rm escpost render example-jobs/cafe-order-voucher.hex --output-dir .test-output/out
```

## Golden images

Renderer tests compare decoded pixels against version-controlled
`expected-NNN.png` fixtures under `crates/escpost-render/tests/cases/<case>/` (and
`crates/escpost-profiles/profiles/<id>/`
for calibration). Tests never rewrite expectations. When a rendering change is
intentional, review the regenerated `.test-output/<case>/actual-NNN.png`
by eye, then copy it over the matching `expected-NNN.png` to accept it. Never
bless a golden solely because the implementation produced it — see
`docs/TESTING.md`.

## Where to read more

- `docs/ARCHITECTURE.md` — crate layout and the render pipeline.
- `docs/DESIGN_DECISIONS.md` — accepted decisions and their rationale (e.g. DD-023,
  the bundled-font policy).
- `docs/TESTING.md` — test layers, golden workflow, and physical-printer calibration.
- `docs/CODING_STYLE.md` — style plus the fmt/clippy/test gate.
