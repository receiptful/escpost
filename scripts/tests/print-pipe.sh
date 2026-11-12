#!/usr/bin/env bash
# Run on a Linux development host with Docker Compose and util-linux script.
# All Python and ESCPost execution stays inside Compose containers.
set -euo pipefail
if [[ $# == 0 ]]; then
    for scenario in select cancel non-interactive unknown-printer redirected-output terminated; do
        bash "$0" "$scenario"
    done
    exit
fi
scenario=$1
case "$scenario" in select|cancel|non-interactive|unknown-printer|redirected-output|terminated) ;; *) exit 2 ;; esac
cd "$(dirname "$0")/../.."
mkdir -p target
fixture=$(mktemp -d target/print-pipe-test.XXXXXX)
server=''
terminal=''
cleanup() {
    if [[ -n "$terminal" ]]; then kill "$terminal" 2>/dev/null || true; fi
    if [[ -n "$server" ]]; then docker rm -f "$server" >/dev/null 2>&1 || true; fi
    rm -rf "$fixture"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
printf '\033@hello\000\377\r\n\032' > "$fixture/receipt.bin"
dd if=/dev/zero bs=65536 count=2 status=none >> "$fixture/receipt.bin"
server=$(docker compose run --rm --no-deps -d --entrypoint python3 test -c '
import pathlib, socket, sys
directory = pathlib.Path(sys.argv[1])
with socket.socket() as listener:
    listener.bind(("127.0.0.1", 0))
    listener.listen()
    (directory / "port").write_text(str(listener.getsockname()[1]))
    connection, _ = listener.accept()
    with connection, (directory / "received.bin").open("wb") as output:
        while data := connection.recv(65536):
            output.write(data)
    (directory / "done").touch()
' "/workspace/$fixture")
for ((attempt = 0; attempt < 300; attempt++)); do
    [[ -f "$fixture/port" ]] && break
    sleep 0.1
done
[[ -f "$fixture/port" ]] || { echo 'Fake printer did not start.' >&2; exit 1; }
cat > "$fixture/printers.toml" <<EOF
[counter]
transport = "network"
host = "127.0.0.1"
port = 1
[kitchen]
transport = "network"
host = "127.0.0.1"
port = $(cat "$fixture/port")
EOF
export ESCPOST_TEST_RECEIPT="$PWD/$fixture/receipt.bin"
export ESCPOST_TEST_CONFIG="/workspace/$fixture/printers.toml"
export ESCPOST_TEST_SCENARIO="$scenario"
export ESCPOST_TEST_WRAPPER_PID="$PWD/$fixture/wrapper-pid"
mkfifo "$fixture/keys"
exec 3<>"$fixture/keys"
# script otherwise uses the developer's $SHELL to interpret this POSIX script.
SHELL=/bin/sh timeout 180 script --quiet --return --flush --command \
    'case "$ESCPOST_TEST_SCENARIO" in
       select) cat "$ESCPOST_TEST_RECEIPT" | ./escpost print - --config "$ESCPOST_TEST_CONFIG" ;;
       cancel) yes receipt | ./escpost print - --config "$ESCPOST_TEST_CONFIG" ;;
       non-interactive) yes receipt | ./escpost print - --config "$ESCPOST_TEST_CONFIG" --non-interactive ;;
       unknown-printer) yes receipt | ./escpost print - --config "$ESCPOST_TEST_CONFIG" --printer missing ;;
       redirected-output) cat "$ESCPOST_TEST_RECEIPT" | ./escpost print - --config "$ESCPOST_TEST_CONFIG" --printer kitchen > /dev/null ;;
       terminated) yes receipt | ./escpost print - --config "$ESCPOST_TEST_CONFIG" &
         wrapper=$!; echo "$wrapper" > "$ESCPOST_TEST_WRAPPER_PID"; wait "$wrapper" ;;
     esac' \
    /dev/null < "$fixture/keys" > "$fixture/screen" 2>&1 3>&- &
terminal=$!
if [[ "$scenario" == select || "$scenario" == cancel || "$scenario" == terminated ]]; then
    for ((attempt = 0; attempt < 1800; attempt++)); do
        grep -q 'Add a printer' "$fixture/screen" && break
        if ! kill -0 "$terminal" 2>/dev/null; then
            cat "$fixture/screen" >&2
            echo 'FAIL: piped wrapper did not offer printer selection.' >&2
            exit 1
        fi
        sleep 0.1
    done
    grep -q 'Add a printer' "$fixture/screen" || { echo 'Picker timed out.' >&2; exit 1; }
    # counter is first; select kitchen, whose listener records the receipt.
    if [[ "$scenario" == select ]]; then
        printf '\033[B\r' >&3
    elif [[ "$scenario" == cancel ]]; then
        printf '\033' >&3
    else
        kill -TERM "$(cat "$fixture/wrapper-pid")"
    fi
fi
result=0
wait "$terminal" || result=$?
terminal=''
if [[ "$scenario" != select && "$scenario" != redirected-output ]]; then
    [[ "$result" != 0 && "$result" != 124 ]] || { cat "$fixture/screen" >&2; exit 1; }
    [[ ! -e "$fixture/received.bin" ]]
    case "$scenario" in
        non-interactive) grep -q 'multiple printers configured' "$fixture/screen" ;;
        unknown-printer) grep -q 'missing' "$fixture/screen" ;;
        terminated)
            [[ "$result" == 143 ]] || {
                cat "$fixture/screen" >&2
                echo "Expected termination status 143, got $result." >&2
                exit 1
            }
            ;;
    esac
    echo "PASS: $scenario exits without printing or waiting for stdin EOF."
    exit
fi
[[ "$result" == 0 ]] || { cat "$fixture/screen" >&2; exit 1; }
for ((attempt = 0; attempt < 100; attempt++)); do
    [[ -f "$fixture/done" ]] && break
    sleep 0.1
done
cmp "$fixture/receipt.bin" "$fixture/received.bin"
echo "PASS: $scenario preserves binary receipt bytes through the wrapper."
