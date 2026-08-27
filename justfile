# Build, test, and run escpost either in Docker (reproducible, no host toolchain)
# or natively (host Rust toolchain; runs as a real host binary). See README.md.
# Every recipe here has an identical `make` target.

# Containerized cargo, via the compose `test` service.
docker_cargo := "docker compose run --rm test cargo"

# List available recipes.
default:
    @just --list

# --- Docker (no host toolchain) ---

# Compile the CLI in the container.
docker-build:
    {{docker_cargo}} build -p escpost

# Run the test suite in the container.
docker-test:
    {{docker_cargo}} test --workspace --exclude escpost-python
    scripts/test-development-wrapper

# Run the CLI in the container, e.g. `just docker-run serve --no-open`.
docker-run *args:
    {{docker_cargo}} run -q -p escpost -- {{args}}

# Run the backend and Vite development server in Docker.
docker-web-dev:
    docker compose up

# --- Native (host Rust toolchain) ---

# Install the frontend dependencies. The Vite server needs them.
frontend-install:
    cd crates/escpost/frontend && bun install --frozen-lockfile

# Build the web app bundle into crates/escpost/frontend/dist.
frontend-build:
    cd crates/escpost/frontend && bun install --frozen-lockfile && bun run build

# Build target/release/escpost.
native-build: frontend-build
    cargo build --release -p escpost

# Run the test suite on the host.
native-test: frontend-build
    cargo test --workspace --exclude escpost-python
    scripts/test-development-wrapper

# A debug build reads the web app from disk at run time. Use `frontend-build`
# first if the CLI must serve it.
[doc("Run the CLI on the host, e.g. `just native-run serve`.")]
native-run *args:
    cargo run -q -p escpost -- {{args}}

# Run the backend and Vite development server with host toolchains.
native-web-dev: frontend-install
    scripts/native-web-dev

# --- Utilities ---

# Regenerate crates/escpost-profiles/profiles/.generated/profiles.json.
pack:
    {{docker_cargo}} run -q -p escpost-profiles --bin compile-profile-pack -- crates/escpost-profiles/profiles/.escpos-printer-db/dist/capabilities.json crates/escpost-profiles/profiles crates/escpost-profiles/profiles/.generated/profiles.json

# Build and test the Python render binding.
python-test:
    scripts/python-binding-test

# Publish escpost-render and escpost-profiles first, because escpost needs
# them at the versions in this workspace.
[doc("Publish the CLI to crates.io with the web app built in.")]
publish: frontend-build
    cargo publish -p escpost
