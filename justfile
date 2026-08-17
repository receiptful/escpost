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

# Build target/release/escpost.
native-build:
    scripts/frontend-build
    cargo build --release -p escpost

# Run the test suite on the host.
native-test:
    scripts/frontend-build
    cargo test --workspace --exclude escpost-python
    scripts/test-development-wrapper

# Run the CLI on the host, e.g. `just native-run serve`.
native-run *args:
    scripts/frontend-build
    cargo run -q -p escpost -- {{args}}

# Run the backend and Vite development server with host toolchains.
native-web-dev:
    scripts/native-web-dev

# --- Utilities ---

# Regenerate crates/escpost-profiles/profiles/.generated/profiles.json.
pack:
    {{docker_cargo}} run -q -p escpost-profiles --bin compile-profile-pack -- crates/escpost-profiles/profiles/.escpos-printer-db/dist/capabilities.json crates/escpost-profiles/profiles crates/escpost-profiles/profiles/.generated/profiles.json

# Build and test the Python render binding.
python-test:
    scripts/python-binding-test
