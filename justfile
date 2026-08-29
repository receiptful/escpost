# Native development workflows and repository maintenance utilities. Container
# development uses Docker Compose directly; see README.md.

# List available recipes.
default:
    @just --list

# --- Host toolchains ---

# Install the frontend dependencies. The Vite server needs them.
frontend-install:
    cd crates/escpost/frontend && bun install --frozen-lockfile

# Build the web app bundle into crates/escpost/frontend/dist.
frontend-build:
    cd crates/escpost/frontend && bun install --frozen-lockfile && bun run build

# Build target/release/escpost.
build: frontend-build
    cargo build --release -p escpost

# Run the test suite on the host.
test: frontend-build
    cargo test --workspace --exclude escpost-python

# A debug build reads the web app from disk at run time. Use `frontend-build`
# first if the CLI must serve it.
[doc("Run the CLI on the host, e.g. `just run serve`.")]
run *args:
    cargo run -p escpost -- {{args}}

# Run the native development stack, with Rust auto-restart when Watchexec exists.
dev: frontend-install
    scripts/native-dev

# --- Utilities ---

# Clear the shared Docker Cargo build cache.
docker-cargo-clean:
    docker compose run --rm --no-deps --entrypoint sh escpost -c 'find "$CARGO_TARGET_DIR" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +'

# Set the lockstep workspace version and refresh Cargo.lock.
[doc("Set every publishable Rust crate to one release version.")]
set-version version:
    python3 scripts/set-workspace-version {{quote(version)}}
    cargo metadata --format-version 1 --no-deps > /dev/null

# Regenerate crates/escpost-profiles/profiles/.generated/profiles.json.
generate-profile-pack:
    docker compose run --rm --entrypoint cargo escpost run -q -p escpost-profiles --bin compile-profile-pack -- crates/escpost-profiles/profiles/.escpos-printer-db/dist/capabilities.json crates/escpost-profiles/profiles crates/escpost-profiles/profiles/.generated/profiles.json

# Cargo treats ignored packaged `dist/` as dirty; this check makes `--allow-dirty` safe.
[private]
prepare-publish:
    @test -z "$(git status --porcelain --untracked-files=all)" || { echo "Refusing to publish from a dirty worktree."; git status --short; exit 1; }
    just frontend-build

# Publish the crates in dependency order and wait for each one to reach the
# registry index before publishing its dependents.
[doc("Verify every release crate without uploading to crates.io.")]
publish-dry-run: prepare-publish
    cargo publish --workspace --exclude escpost-python --locked --allow-dirty --dry-run

# Set the release version, test it, record it in git, and upload it. The
# working tree must be clean first, so the only change committed is the version
# itself. Everything before the prompt is reversible; nothing after it is,
# because crates.io offers yank rather than delete.
#
# Resumable: each step is skipped when it is already done, so a release that
# fails partway can be rerun without unpicking what already succeeded.
[doc("Set the version, test, tag, push, and publish to crates.io.")]
publish version: (_require-release-branch) (_require-clean-worktree) (set-version version) test
    @git --no-pager diff --stat
    @printf '\nPublish {{version}} to crates.io? Uploads cannot be undone. [y/N] '; \
     read -r reply; \
     case "$reply" in \
       [yY]|[yY][eE][sS]) ;; \
       *) echo "Aborted. The version change is left in your working tree."; exit 1 ;; \
     esac
    @git diff --quiet || git commit -am "escpost {{version}}"
    @git rev-parse -q --verify "refs/tags/v{{version}}" > /dev/null \
      || git tag "v{{version}}"
    git push
    git push --tags
    scripts/publish-crates {{version}}

# Releases are cut from main: the recipe commits, tags, and pushes, so running
# it elsewhere would tag the wrong history. Being behind the remote is refused
# too, because the push after the commit would be rejected, leaving a version
# commit and tag stranded locally.
[private]
_require-release-branch:
    @branch="$(git branch --show-current)"; \
      test "$branch" = "main" \
        || { echo "Refusing to release from '$branch'; releases are cut from main."; exit 1; }
    @git fetch --quiet origin
    @behind="$(git rev-list --count HEAD..origin/main)"; \
      test "$behind" = "0" \
        || { echo "Refusing to release: main is $behind commit(s) behind origin/main."; \
             echo "Run 'git pull --rebase' first."; exit 1; }

[private]
_require-clean-worktree:
    @test -z "$(git status --porcelain --untracked-files=all)" \
      || { echo "Refusing to release from a dirty worktree."; git status --short; exit 1; }
