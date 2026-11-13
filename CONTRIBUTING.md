# Contributing

This guide shows how to build, run, and test ESCPost from a source checkout.
Read [`docs/CODING_STYLE.md`](docs/CODING_STYLE.md) and
[`docs/TESTING.md`](docs/TESTING.md) before you send a change.

## Docker Compose workflow

Docker Compose is the canonical development workflow. It requires neither Rust
nor Bun on the host. Start the complete backend and frontend stack with:

```bash
docker compose up
```

Open `http://127.0.0.1:5173/`. Vite serves the frontend there with hot reload
and proxies API requests to the backend. Backend source changes restart the
Rust process.

To provide a discoverable RAW TCP printer without physical hardware, run this
in another terminal:

```bash
docker compose up dummy-printer
```

Use the root wrapper to run the containerized CLI with USB access:

```bash
./escpost printers list
echo hello | ./escpost print
```

For piped printing in a terminal, the wrapper keeps receipt data on a separate
stream so you can use the interactive printer picker without changing the bytes.
When running without a terminal or redirecting the command's output, pass
`--printer <NAME>` if several printers are configured.

Run the Rust workspace tests in the same reproducible environment:

```bash
docker compose run --rm test cargo test --workspace
```

For a production-like run of the embedded frontend without development
watchers, use `docker compose run --rm -e ESCPOST_WATCH=0 escpost serve`.

[`AGENTS.md`](AGENTS.md) lists more Compose commands: formatting, lints,
frontend tests, and the golden-image workflow.

## Native workflow

Native development requires Rust and Bun on the host. `just` is optional, but
we recommend installing it to run the repository tasks below; their underlying
commands can also be invoked directly:

| Task | Command |
|---|---|
| Build the release CLI | `just build` |
| Run the tests | `just test` |
| Run the CLI | `just run serve` |
| Run the development stack | `just dev` |

The native build compiles the frontend first and produces
`target/release/escpost`. To install that checkout on `PATH`:

```bash
cargo install --path crates/escpost
```

`just dev` provides Vite HMR and uses Watchexec, when available, to restart the
Rust server after backend changes. If Watchexec is missing, it offers to install
it locally under `target/dev-tools`.

Additional tasks:

- `just docker-cargo-clean` clears the shared container build cache.
- `just generate-profile-pack` regenerates the canonical printer-profile pack.
- `cd python && just test` builds and exercises the Python binding.
- `just --list` shows every recipe.

## Windows packaging

Windows x64 distribution builds use `just packaging windows build` from a
clean release tag, or `just packaging windows build --dev` for a working-tree
build. This produces an unsigned portable ZIP and per-user Inno Setup installer
in `dist/windows/`. Run `just packaging windows verify INSTALLER` to test the
finished installer on a separate clean Windows desktop, or
`just packaging windows tester start --installer INSTALLER` to try it yourself.
See [Windows packaging](packaging/windows/README.md) for
builder requirements, verification, and unsigned-download limitations.

## JavaScript SDK and browser extension

Each has its own tooling and guide: [`sdks/javascript`](sdks/javascript) and
[`browser-extension`](browser-extension).
