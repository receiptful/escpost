#!/usr/bin/env bash

validate_installer() {
    [[ -f "$1" && "$1" == *.[eE][xX][eE] ]] || die "Expected an existing .exe installer: $1"
    [[ "$(head -c 2 "$1")" == MZ ]] || die "Not a Windows executable: $1"
}

require_stopped() {
    [[ -z "$1" ]] || die 'The tester is running or paused. Stop it explicitly before reset or verification; its current session will be discarded.'
}

restore_baseline() {
    local state="$1"
    [[ -f "$state/baseline/ready.json" && -f "$state/baseline/storage/data.img" ]] || die 'No complete tester baseline exists.'
    [[ ! -e "$state/storage.restore" ]] || die 'An interrupted restore exists. Inspect storage.restore before retrying.'
    [[ ! -e "$state/storage.previous" ]] || die 'An interrupted disk swap exists. Inspect storage.previous before retrying.'
    mkdir "$state/storage.restore"
    cp -a --reflink=auto --sparse=always "$state/baseline/storage/." "$state/storage.restore/" || die 'Baseline copy failed; current disk was preserved.'
    # Commit the replacement only after the complete clone exists.
    if [[ -e "$state/storage" ]]; then mv "$state/storage" "$state/storage.previous"; fi
    mv "$state/storage.restore" "$state/storage"
    rm -rf -- "$state/storage.previous"
}
