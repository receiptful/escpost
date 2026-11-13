#!/usr/bin/env bash
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
source "$here/common.sh"
source "$here/tester/usb.sh"
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
sysfs="$scratch/sysfs"
mkdir -p "$sysfs"
expect_failure() { if ("$@") > "$scratch/error" 2>&1; then die "Expected failure: $*"; fi; }
device() {
    local id="$1" class="$2" vendor="$3" product="$4"
    mkdir -p "$sysfs/$id"
    printf '%s\n' "$class" > "$sysfs/$id/bDeviceClass"
    printf '%s\n' "$vendor" > "$sysfs/$id/idVendor"
    printf '%s\n' "$product" > "$sysfs/$id/idProduct"
    printf '%s\n' "${id%%-*}" > "$sysfs/$id/busnum"
    printf '%s\n' "${id#*-}" > "$sysfs/$id/devpath"
    printf 'Printer\033[2J\n' > "$sysfs/$id/product"
}
expect_failure usb_list "$sysfs"
device 3-4 00 0416 5011
device 3-5.2 00 0416 5011
device 3-1 09 1234 5678
mkdir -p "$sysfs/usb3" "$sysfs/3-4:1.0"
usb_scan "$sysfs"
[[ "${usb_devices[*]}" == '3-4 3-5.2' ]]
usb_select "$sysfs" 3-5.2
[[ "$usb_selection" == 3-5.2:0416:5011 && "$usb_bus" == 3 && "$usb_port" == 5.2 ]]
[[ "$(usb_describe "$sysfs")" != *$'\033'* ]]
usb_select "$sysfs" 3-5.2 3-4
[[ "${usb_selected[*]}" == '3-4 3-5.2' ]]
[[ "$usb_selection" == '3-4:0416:5011,3-5.2:0416:5011' ]]
[[ "$usb_arguments" == '-device usb-host,hostbus=3,hostport=4,vendorid=0x0416,productid=0x5011 -device usb-host,hostbus=3,hostport=5.2,vendorid=0x0416,productid=0x5011' ]]
selection=$usb_selection
usb_select "$sysfs" 3-4 3-5.2
[[ "$usb_selection" == "$selection" ]]
expect_failure usb_select "$sysfs" 3-4 3-4
expect_failure usb_select "$sysfs" 0416:5011
expect_failure usb_select "$sysfs" 3-1
expect_failure usb_select "$sysfs" '../3-4'
expect_failure usb_select "$sysfs" '3-4,hostbus=1'
expect_failure usb_select "$sysfs" </dev/null
# The picker accepts several numbered entries, never silently a partial choice.
usb_scan "$sysfs"
usb_parse_choices '2, 1'
[[ "${usb_selected[*]}" == '3-5.2 3-4' ]]
for choice in '1 1' '0' '3' '1 nope' '9999999999999999999' '' '1,,2'; do
    expect_failure usb_parse_choices "$choice"
done
rm "$sysfs/3-5.2/idProduct"
expect_failure usb_select "$sysfs" 3-4 3-5.2
device 3-5.2 00 0483 5743
# Revalidation detects replacement or removal of any selected device.
usb_select "$sysfs" 3-4 3-5.2
device 3-5.2 00 1234 5678
expect_failure usb_revalidate "$sysfs"
device 3-5.2 00 0483 5743
usb_select "$sysfs" 3-4 3-5.2
rm "$sysfs/3-4/idProduct"
expect_failure usb_revalidate "$sysfs"
device 3-4 00 0416 5011
usb_require_same_attachment '' '' 3-4:0416:5011
usb_require_same_attachment running 3-4:0416:5011 3-4:0416:5011
expect_failure usb_require_same_attachment running '' 3-4:0416:5011
expect_failure usb_require_same_attachment running 3-4:0416:5011 ''

# Exercise the real runner with a fake Docker/SSH boundary: refusal must happen
# before compose up, and USB must not leak into ordinary starts or reset.
runner="$scratch/windows/tester"
mkdir -p "$runner/.state/baseline/storage" "$runner/.state/ssh" "$scratch/bin" "$scratch/windows/.state/media"
cp "$here/common.sh" "$scratch/windows/common.sh"
cp "$here/tester/"{run,common.sh,usb.sh,compose.yaml,compose.usb.yaml} "$runner/"
sed -i "s@/sys/bus/usb/devices@$sysfs@g" "$runner/usb.sh" "$runner/run"
printf '{}' > "$runner/.state/baseline/ready.json"
printf clean > "$runner/.state/baseline/storage/data.img"
touch "$runner/.state/ssh/controller" "$runner/.state/ssh/controller.pub" "$scratch/windows/.state/media/windows.iso"
printf 'ESCPOST_TESTER_SSH_PORT=2233\nESCPOST_TESTER_VIEWER_PORT=8027\n' > "$runner/.state/compose.env"
cat > "$scratch/bin/docker" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$CALLS"
case "$1" in
    context) echo "${MOCK_ENDPOINT:-unix:///var/run/docker.sock}" ;;
    info) echo "${MOCK_OS:-Linux}" ;;
    inspect)
        if [[ "$*" == *'.State.Running'* ]]; then echo "$RUNNING"
        else printf '%s\n' "$ATTACHMENT"; fi ;;
    compose)
        if [[ "$*" == *' ps --all -q windows' && -n "$RUNNING" ]]; then echo container; fi
        if [[ "$*" == *' up -d'* ]]; then
            printf 'USB arguments: %s\n' "${ESCPOST_TESTER_USB_ARGUMENTS:-}" >> "$CALLS"
            mkdir -p "$MOCK_STATE/storage"
            printf clean > "$MOCK_STATE/storage/data.img"
        fi ;;
esac
SH
printf '#!/bin/sh\nexit 0\n' > "$scratch/bin/ssh"
cp "$scratch/bin/ssh" "$scratch/bin/scp"
cp "$scratch/bin/ssh" "$scratch/bin/sleep"
chmod +x "$scratch/bin/"*
export PATH="$scratch/bin:$PATH" CALLS="$scratch/calls" RUNNING=container ATTACHMENT=''
export MOCK_STATE="$runner/.state"
unset DOCKER_HOST DOCKER_CONTEXT
: > "$CALLS"
expect_failure "$runner/run" start --usb 3-4
! grep -q ' up ' "$CALLS"
export ATTACHMENT=3-4:0416:5011
"$runner/run" start --usb 3-4 > /dev/null
grep -q 'compose.usb.yaml up -d --no-recreate' "$CALLS"
: > "$CALLS"
expect_failure "$runner/run" start --usb 3-4 --usb 3-5.2
! grep -q ' up ' "$CALLS"
export ATTACHMENT='3-4:0416:5011,3-5.2:0483:5743'
"$runner/run" start --usb 3-5.2 --usb 3-4 > /dev/null
grep -q 'compose.usb.yaml up -d --no-recreate' "$CALLS"
grep -q 'USB arguments: -device usb-host,hostbus=3,hostport=4,vendorid=0x0416,productid=0x5011 -device usb-host,hostbus=3,hostport=5.2,vendorid=0x0483,productid=0x5743' "$CALLS"
: > "$CALLS"
expect_failure "$runner/run" start --usb 3-4
! grep -q ' up ' "$CALLS"
expect_failure "$runner/run" start --usb 3-4 --usb 3-4
expect_failure "$runner/run" start --usb 3-4 --usb
! grep -q ' up ' "$CALLS"
: > "$CALLS"
expect_failure "$runner/run" start
! grep -q ' up ' "$CALLS"
export RUNNING=''
"$runner/run" start --usb 3-4 > /dev/null
grep -q 'compose.usb.yaml up -d' "$CALLS"
: > "$CALLS"
"$runner/run" start > /dev/null
! grep -q 'compose.usb.yaml' "$CALLS"
"$runner/run" reset > /dev/null
[[ "$(cat "$runner/.state/storage/data.img")" == clean ]]
! grep -q 'compose.usb.yaml' "$CALLS"
expect_failure "$runner/run" reset --usb 3-4

# Bootstrap must seal a USB-free disk even when the first start requests USB.
rm -rf "$runner/.state/baseline"
mkdir -p "$runner/oem"
touch "$runner/oem/agent.ps1"
: > "$CALLS"
"$runner/run" start --usb 3-4 > /dev/null
mapfile -t starts < <(grep ' up -d' "$CALLS")
[[ ${#starts[@]} == 2 && "${starts[0]}" != *compose.usb.yaml* && "${starts[1]}" == *compose.usb.yaml* ]]
[[ "$(cat "$runner/.state/baseline/storage/data.img")" == clean ]]

# Verification still restores and starts clean Windows without USB. The
# acceptance runner itself has separate lifecycle/transport contract tests.
printf 'printf "acceptance started\\n" >> "$CALLS"\n' > "$runner/verify.sh"
printf MZ > "$scratch/candidate.exe"
: > "$CALLS"
"$runner/run" verify "$scratch/candidate.exe" > /dev/null
grep -q 'acceptance started' "$CALLS"
! grep -q 'compose.usb.yaml' "$CALLS"

for engine in remote-host remote-context desktop; do
    : > "$CALLS"
    case "$engine" in
        remote-host) export DOCKER_HOST=tcp://remote:2376 ;;
        remote-context) export DOCKER_CONTEXT=remote MOCK_ENDPOINT=ssh://remote ;;
        desktop) export MOCK_OS='Docker Desktop' ;;
    esac
    expect_failure "$runner/run" start --usb 3-4
    ! grep -q ' up ' "$CALLS"
    unset DOCKER_HOST DOCKER_CONTEXT MOCK_ENDPOINT MOCK_OS
done
printf 'USB selection and lifecycle checks passed.\n'
