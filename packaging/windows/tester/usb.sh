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

usb_select() {
    local requested="$1" sysfs="${2:-/sys/bus/usb/devices}" index choice
    if [[ -z "$requested" ]]; then
        [[ -t 0 ]] || die 'The USB picker needs a terminal. Use tester usb-list, then start --usb DEVICE.'
        usb_scan "$sysfs"
        ((${#usb_devices[@]})) || die 'No selectable USB devices found on this Linux host.'
        printf 'Select the USB device to give Windows (q cancels):\n' >&2
        for index in "${!usb_devices[@]}"; do
            usb_read_device "${usb_devices[$index]}" "$sysfs" || continue
            printf '%d) ' "$((index + 1))" >&2
            usb_describe "$sysfs" >&2
        done
        while true; do
            printf 'Device number: ' >&2
            read -r choice || die 'USB selection cancelled.'
            [[ "$choice" != q && "$choice" != Q ]] || die 'USB selection cancelled.'
            if [[ "$choice" =~ ^[1-9][0-9]{0,5}$ ]] && ((choice <= ${#usb_devices[@]})); then
                requested=${usb_devices[$((choice - 1))]}
                break
            fi
            printf 'Choose a listed number, or q to cancel.\n' >&2
        done
    fi
    usb_read_device "$requested" "$sysfs" || die "USB device '$requested' is absent, is a hub, or has invalid descriptors. Run tester usb-list again."
    usb_selection="$usb_id:$usb_vendor:$usb_product"
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
