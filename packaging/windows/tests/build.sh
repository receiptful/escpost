#!/usr/bin/env bash
set -euo pipefail
packaging=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=packaging/windows/common.sh
source "$packaging/common.sh"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
root="$scratch/checkout with spaces"
job="$scratch/job"
mkdir -p "$root" "$job"
git -C "$root" init -q
git -C "$root" config user.name 'Packaging test'
git -C "$root" config user.email packaging-test@example.invalid
cat > "$root/Cargo.toml" <<'EOF'
[workspace.package]
version = "1.2.3"
EOF
cat > "$root/.gitignore" <<'EOF'
.config/
dist/
.state/
EOF
mkdir -p "$root/crates/example/src" "$root/packaging/windows/builder/.state"
printf 'fn main() {}\n' > "$root/crates/example/src/main.rs"
git -C "$root" add .
git -C "$root" commit -qm initial
git -C "$root" tag v1.2.3
[[ "$(build_version "$root" release)" == 1.2.3 ]]
[[ "$(build_version "$root" dev)" == 1.2.3-dev.* ]]

expect_failure() {
    if ("$@") > "$scratch/failure.log" 2>&1; then die "Expected failure: $*"; fi
}
printf '\n// changed\n' >> "$root/crates/example/src/main.rs"
expect_failure build_version "$root" release
[[ "$(build_version "$root" dev)" == *-dirty ]]
git -C "$root" add .
git -C "$root" commit -qm changed
expect_failure build_version "$root" release

mkdir -p "$root/crates/escpost/frontend/dist/assets" "$root/crates/escpost/frontend/src" "$root/example-jobs" "$root/.config" "$root/dist"
printf 'frontend test input\n' > "$root/crates/escpost/frontend/src/app.tsx"
printf '1b 40\n' > "$root/example-jobs/job.hex"
printf 'bundled HTML\n' > "$root/crates/escpost/frontend/dist/index.html"
printf 'bundled JS\n' > "$root/crates/escpost/frontend/dist/assets/app.js"
printf 'secret\n' > "$root/.config/printers.toml"
printf 'secret\n' > "$root/crates/example/.env"
printf 'secret\n' > "$root/packaging/windows/builder/.state/private-key"
printf 'old artifact\n' > "$root/dist/old.exe"
printf 'fn added() {}\n' > "$root/crates/example/src/added.rs"
stage_sources "$root" "$job"
mkdir "$scratch/extracted"
tar -xzf "$job/source.tar.gz" -C "$scratch/extracted"
[[ -f "$scratch/extracted/crates/escpost/frontend/dist/assets/app.js" ]]
[[ -f "$scratch/extracted/crates/escpost/frontend/src/app.tsx" ]]
[[ -f "$scratch/extracted/example-jobs/job.hex" ]]
[[ -f "$scratch/extracted/crates/example/src/added.rs" ]]
[[ ! -e "$scratch/extracted/.config" ]]
[[ ! -e "$scratch/extracted/dist" ]]
[[ ! -e "$scratch/extracted/crates/example/.env" ]]
[[ ! -e "$scratch/extracted/packaging/windows/builder/.state" ]]
cmp "$root/crates/example/src/main.rs" "$scratch/extracted/crates/example/src/main.rs"
(cd "$job" && sha256sum --check source.sha256)
ln -s "$root/.config/printers.toml" "$root/crates/example/src/linked.rs"
expect_failure stage_sources "$root" "$job"
rm "$root/crates/example/src/linked.rs" "$root/crates/escpost/frontend/dist/index.html"
expect_failure stage_sources "$root" "$job"
printf 'Packaging contract checks passed.\n'
