# Native development workflows and repository maintenance utilities. Container
# development uses Docker Compose directly; see README.md.

# Browser extension recipes: `just --list browser-extension`.
mod browser-extension

# JavaScript SDK recipes: `just --list javascript-sdk`.
mod javascript-sdk 'sdks/javascript'

# Native distribution artifacts: `just --list packaging windows`.
mod packaging

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

# Set the lockstep product version and refresh Cargo.lock.
[doc("Set the Rust workspace, browser SDK, and extension to one release version.")]
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
    @printf '\nRelease {{version}} will:\n'; \
     printf '  - commit the version bump and tag it v{{version}}\n'; \
     printf '  - push the commit and the tag to origin\n'; \
     printf '  - upload the crates to crates.io, which cannot be undone\n'; \
     printf '\nProceed? [y/N] '; \
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
    @printf '\nReleased {{version}}:\n'; \
     printf '  - committed the version bump and tagged v{{version}}\n'; \
     printf '  - pushed the commit and the tag to origin\n'; \
     printf '  - published the crates to crates.io\n'; \
     printf '\nNext: run `just release {{version}}` in receiptful/homebrew-tap\n'; \
     printf 'to point the Homebrew formula at this release.\n'; \
     printf 'For Windows: `just packaging windows build`, verify the installer,\n'; \
     printf 'then `just gh-release {{version}}`.\n'

# Cut the GitHub release for a tag that `just publish` already pushed, and
# attach the Windows artifacts that `just packaging windows build` made from
# that tag. It builds nothing: a release filename has no `-dev` suffix, and the
# build makes such a file only from a clean checkout at the tag.
#
# Resumable: the release is created when it is missing; a second run replaces
# the attached files.
[doc("Create the GitHub release for a pushed tag and attach the Windows installer.")]
gh-release version:
    #!/usr/bin/env bash
    set -euo pipefail
    version={{quote(version)}}
    tag="v$version"
    directory=dist/windows
    base="escpost-$version-windows-x64"
    files=("$base-setup.exe" "$base.zip" "$base-SHA256SUMS")

    for file in "${files[@]}"; do
        test -f "$directory/$file" || {
            echo "Missing $directory/$file."
            echo
            echo "Build and verify the Windows artifacts from the tag, then run this again:"
            echo
            echo "    git checkout $tag"
            echo "    just packaging windows build"
            echo "    just packaging windows verify $directory/$base-setup.exe"
            echo "    just gh-release $version"
            exit 1
        }
    done
    (cd "$directory" && sha256sum --check --quiet "$base-SHA256SUMS")
    git ls-remote --exit-code --tags origin "refs/tags/$tag" > /dev/null || {
        echo "Tag $tag is not on origin. Run 'just publish $version' first."
        exit 1
    }

    assets=("${files[@]/#/$directory/}")
    if gh release view "$tag" > /dev/null 2>&1; then
        gh release upload "$tag" "${assets[@]}" --clobber
    else
        gh release create "$tag" "${assets[@]}" --verify-tag --title "$version" \
            --generate-notes --notes "The Windows installer and ZIP are not code-signed. SmartScreen can show a warning for an unknown publisher; see [packaging/windows/README.md](https://github.com/receiptful/escpost/blob/$tag/packaging/windows/README.md)."
    fi
    gh release view "$tag" --json url --jq .url

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
