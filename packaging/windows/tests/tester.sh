#!/usr/bin/env bash
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=packaging/windows/common.sh
source "$here/common.sh"
# shellcheck source=packaging/windows/tester/common.sh
source "$here/tester/common.sh"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
expect_failure() { if ("$@") > "$scratch/error" 2>&1; then die "Expected failure: $*"; fi; }
expect_failure validate_installer "$scratch/missing.exe"
printf text > "$scratch/not.exe"
expect_failure validate_installer "$scratch/not.exe"
printf MZ > "$scratch/installer with spaces.exe"
validate_installer "$scratch/installer with spaces.exe"
require_stopped ''
expect_failure require_stopped running-container
mkdir -p "$scratch/vm/storage"
printf original > "$scratch/vm/storage/data.img"
expect_failure restore_baseline "$scratch/vm"
[[ "$(cat "$scratch/vm/storage/data.img")" == original ]]
mkdir -p "$scratch/vm/baseline/storage"
printf clean > "$scratch/vm/baseline/storage/data.img"
printf firmware > "$scratch/vm/baseline/storage/windows.vars"
printf '{}' > "$scratch/vm/baseline/ready.json"
restore_baseline "$scratch/vm"
[[ "$(cat "$scratch/vm/storage/data.img")" == clean ]]
[[ "$(cat "$scratch/vm/storage/windows.vars")" == firmware ]]
printf changed > "$scratch/vm/storage/data.img"
[[ "$(cat "$scratch/vm/baseline/storage/data.img")" == clean ]]
mkdir "$scratch/vm/storage.restore"
expect_failure restore_baseline "$scratch/vm"
[[ "$(cat "$scratch/vm/storage/data.img")" == changed ]]
rmdir "$scratch/vm/storage.restore"
mkdir "$scratch/vm/storage.previous"
expect_failure restore_baseline "$scratch/vm"
[[ "$(cat "$scratch/vm/storage/data.img")" == changed ]]
# A failed control connection must retain a report and attempt evidence export,
# even when failure happens before the guest has created its result directory.
cat > "$scratch/transport-failure.sh" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
here=$1 state=$2 failure_mode=$3
installer="$state/candidate.exe" previous='' viewer=8027
remote() {
    if [[ "$failure_mode" == early || "$*" == *'-Action Stage'* ]]; then return 42; fi
    if [[ "$*" == *'-Action Stop'* ]]; then touch "$state/task-stop-attempted"; fi
}
transfer() {
    if [[ "$1" == -r ]]; then touch "$state/evidence-attempted"; return 1; fi
}
source "$here/verify.sh"
SCRIPT
for failure_mode in early staged; do
    transport="$scratch/transport-$failure_mode"
    mkdir -p "$transport/runs" "$transport/input"
    cp "$scratch/installer with spaces.exe" "$transport/candidate.exe"
    set +e
    bash "$scratch/transport-failure.sh" "$here/tester" "$transport" "$failure_mode" > "$transport/run.log" 2>&1
    code=$?
    set -e
    [[ "$code" == 42 && -f "$transport/evidence-attempted" ]]
    [[ "$failure_mode" != staged || -f "$transport/task-stop-attempted" ]]
    reports=("$transport/runs/"*/host-failure.json)
    [[ "${#reports[@]}" == 1 ]]
    grep -q '"exitCode":42' "${reports[0]}"
done
printf 'Tester lifecycle contract checks passed.\n'
