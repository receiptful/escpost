#!/usr/bin/env bash
# Shared by the entry point, VM runner, and packaging contract tests.

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

lock_builder() {
    local state="$1"
    mkdir -p "$state"
    chmod 700 "$state"
    # Reuse the entry point's lock when it invokes the VM runner.
    if [[ "$(readlink /proc/$$/fd/9 2>/dev/null || true)" != "$state/build.lock" ]]; then
        exec 9>"$state/build.lock"
    fi
    flock -n 9 || die 'Another build or VM operation is using this builder.'
}

build_version() {
    local root="$1" mode="$2" version revision dirty
    version=$(awk '
        /^\[workspace.package\]$/ { package = 1; next }
        /^\[/ { package = 0 }
        package && /^version = / { gsub(/"/, "", $3); print $3 }
    ' "$root/Cargo.toml")
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die 'Expected a numeric workspace version (X.Y.Z).'
    revision=$(git -C "$root" rev-parse --short=12 HEAD)
    dirty=$(git -C "$root" status --porcelain --untracked-files=all)
    if [[ "$mode" == release ]]; then
        [[ -z "$dirty" ]] || die 'Release builds need a clean checkout. Use --dev for a development build.'
        [[ "$(git -C "$root" rev-parse --verify "refs/tags/v${version}^{commit}" 2>/dev/null)" == \
            "$(git -C "$root" rev-parse HEAD)" ]] || die "Release builds require tag v$version at HEAD. Use --dev otherwise."
        printf '%s\n' "$version"
    else
        printf '%s-dev.%s%s\n' "$version" "$revision" "${dirty:+-dirty}"
    fi
}

stage_sources() {
    local root="$1" job="$2" path
    [[ -f "$root/crates/escpost/frontend/dist/index.html" ]] || die 'The frontend bundle is missing.'
    # Git supplies only tracked/non-ignored project inputs. Do not recursively
    # archive a checkout: it can contain printer credentials and large VM disks.
    git -C "$root" ls-files --cached --others --exclude-standard -z -- \
        Cargo.toml Cargo.lock LICENSE crates example-jobs python/rust python/src/escpost/licenses packaging/windows > "$job/candidates"
    : > "$job/files"
    while IFS= read -r -d '' path; do
        case "$path" in
            crates/escpost/frontend/dist/*|*/.state/*|*/.env|*/.env.*|*/node_modules/*|*/target/*|*/.venv/*) continue ;;
        esac
        [[ ! -L "$root/$path" ]] || die "Source symlinks are not supported: $path"
        [[ -f "$root/$path" ]] && printf '%s\0' "$path" >> "$job/files"
    done < "$job/candidates"
    # The one ignored input required by rust-embed is added explicitly.
    (cd "$root" && find crates/escpost/frontend/dist -type f -print0) >> "$job/files"
    [[ -z "$(find "$root/crates/escpost/frontend/dist" -type l -print -quit)" ]] || die 'Frontend bundle contains a symlink.'
    tar -czf "$job/source.tar.gz" -C "$root" --null --verbatim-files-from -T "$job/files"
    (cd "$job" && sha256sum source.tar.gz > source.sha256)
    rm "$job/candidates" "$job/files"
}
