#!/usr/bin/env bash
# Linux sysfs identifies a physical USB port independently of the transient
# device address. Include VID/PID in QEMU's filter so a different device plugged
# into that port cannot be attached accidentally during this Windows session.

usb_read_device() {
    local identifier="$1" sysfs="${2:-/sys/bus/usb/devices}" directory
    [[ "$identifier" =~ ^[1-9][0-9]*-[1-9][0-9]*(\.[1-9][0-9]*)*$ ]] || return 1
    directory="$sysfs/$identifier"
    [[ -r "$directory/idVendor" && -r "$directory/idProduct" &&
        -r "$directory/busnum" && -r "$directory/devpath" &&
        -r "$directory/bDeviceClass" ]] || return 1
    [[ "$(cat "$directory/bDeviceClass")" != 09 ]] || return 1
    usb_vendor=$(cat "$directory/idVendor")
    usb_product=$(cat "$directory/idProduct")
    usb_bus=$(cat "$directory/busnum")
    usb_port=$(cat "$directory/devpath")
    [[ "$usb_vendor" =~ ^[0-9a-fA-F]{4}$ && "$usb_product" =~ ^[0-9a-fA-F]{4}$ &&
        "$usb_bus" =~ ^[1-9][0-9]*$ && "$usb_port" =~ ^[1-9][0-9]*(\.[1-9][0-9]*)*$ ]] || return 1
    [[ "$identifier" == "$usb_bus-$usb_port" ]] || return 1
    usb_id="$identifier"
    usb_vendor=${usb_vendor,,}
    usb_product=${usb_product,,}
}

usb_scan() {
    local sysfs="${1:-/sys/bus/usb/devices}" directory
    usb_devices=()
    for directory in "$sysfs/"*; do
        if usb_read_device "${directory##*/}" "$sysfs"; then
            usb_devices+=("$usb_id")
        fi
    done
}

usb_describe() {
    local sysfs="${1:-/sys/bus/usb/devices}" field value
    printf '%s  %s:%s' "$usb_id" "$usb_vendor" "$usb_product"
    for field in manufacturer product; do
        if [[ -r "$sysfs/$usb_id/$field" ]]; then
            # USB strings are device-controlled: never print terminal controls.
            value=$(LC_ALL=C tr -cd '[:print:]' < "$sysfs/$usb_id/$field")
            printf '  %s' "$value"
        fi
    done
    printf '\n'
}

usb_list() {
    local sysfs="${1:-/sys/bus/usb/devices}" identifier
    usb_scan "$sysfs"
    ((${#usb_devices[@]})) || die 'No selectable USB devices found on this Linux host.'
    for identifier in "${usb_devices[@]}"; do
        usb_read_device "$identifier" "$sysfs" || continue
        usb_describe "$sysfs"
    done
}

usb_parse_choices() {
    local choice="$1" number
    local -a numbers=()
    local -A seen=()
    # Commas and/or whitespace separate numbers. Reject malformed lists before
    # resolving any entry, including duplicate devices and numeric overflow.
    [[ "$choice" =~ ^[[:space:]]*[1-9][0-9]{0,5}([[:space:]]*,[[:space:]]*[1-9][0-9]{0,5}|[[:space:]]+[1-9][0-9]{0,5})*[[:space:]]*$ ]] || return 1
    read -r -a numbers <<< "${choice//,/ }"
    usb_selected=()
    for number in "${numbers[@]}"; do
        (( number <= ${#usb_devices[@]} )) || return 1
        [[ ! -v "seen[$number]" ]] || return 1
        seen[$number]=1
        usb_selected+=("${usb_devices[$((number - 1))]}")
    done
}

usb_resolve_selection() {
    local sysfs="$1" identifier
    local -A seen=()
    ((${#usb_selected[@]})) || die 'Select at least one USB device.'
    for identifier in "${usb_selected[@]}"; do
        usb_read_device "$identifier" "$sysfs" || die "USB device '$identifier' is absent, is a hub, or has invalid descriptors. Run tester usb-list again."
        [[ ! -v "seen[$identifier]" ]] || die "USB device '$identifier' was selected more than once."
        seen[$identifier]=1
    done
    # Canonical order makes a reordered selection match an existing VM. Keep
    # the single-device label format compatible with already-running testers.
    mapfile -t usb_selected < <(printf '%s\n' "${usb_selected[@]}" | LC_ALL=C sort)
    usb_selection=''
    usb_arguments=''
    for identifier in "${usb_selected[@]}"; do
        usb_read_device "$identifier" "$sysfs" || die "USB device '$identifier' changed during selection. Select it again."
        usb_selection+="${usb_selection:+,}$usb_id:$usb_vendor:$usb_product"
        usb_arguments+="${usb_arguments:+ }-device usb-host,hostbus=$usb_bus,hostport=$usb_port,vendorid=0x$usb_vendor,productid=0x$usb_product"
    done
}

usb_select() {
    local sysfs="$1" index choice
    shift
    usb_selected=("$@")
    if ((${#usb_selected[@]} == 0)); then
        [[ -t 0 ]] || die 'The USB picker needs a terminal. Use tester usb-list, then repeat --usb DEVICE for each device.'
        usb_scan "$sysfs"
        ((${#usb_devices[@]})) || die 'No selectable USB devices found on this Linux host.'
        printf 'Select USB devices to give Windows (q cancels):\n' >&2
        for index in "${!usb_devices[@]}"; do
            usb_read_device "${usb_devices[$index]}" "$sysfs" || continue
            printf '%d) ' "$((index + 1))" >&2
            usb_describe "$sysfs" >&2
        done
        while true; do
            printf 'Device numbers (for example 1 3 or 1,3): ' >&2
            read -r choice || die 'USB selection cancelled.'
            [[ "$choice" != q && "$choice" != Q ]] || die 'USB selection cancelled.'
            if usb_parse_choices "$choice"; then break; fi
            printf 'Choose distinct listed numbers separated by spaces or commas, or q to cancel.\n' >&2
        done
    fi
    usb_resolve_selection "$sysfs"
}

usb_revalidate() {
    local previous="$usb_selection"
    usb_resolve_selection "$1"
    [[ "$usb_selection" == "$previous" ]] || die 'A device at a selected USB port changed. Select the devices again.'
}

usb_require_same_attachment() {
    local running="$1" current="$2" requested="$3"
    if [[ -n "$running" && "$current" != "$requested" ]]; then
        die 'Changing USB attachment requires a stopped tester. Run just packaging windows tester stop, then repeat start. The Windows disk is preserved.'
    fi
}

usb_require_local_docker() {
    local endpoint operating_system
    [[ "$(uname -s)" == Linux ]] || die 'Tester USB passthrough currently requires a Linux host.'
    if [[ -n "${DOCKER_CONTEXT:-}" || -z "${DOCKER_HOST:-}" ]]; then
        endpoint=$(docker context inspect --format '{{.Endpoints.docker.Host}}') || return
    else
        endpoint=$DOCKER_HOST
    fi
    [[ "$endpoint" == unix://* ]] || die 'USB selection requires a local Linux Docker engine, not a remote Docker host.'
    operating_system=$(docker info --format '{{.OperatingSystem}}') || return
    [[ "$operating_system" != *'Docker Desktop'* ]] || die 'USB selection requires native Linux Docker, not Docker Desktop.'
}
