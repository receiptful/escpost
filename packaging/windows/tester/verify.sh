#!/usr/bin/env bash
# Sourced by run after the clean VM is restored and ready.
# shellcheck disable=SC2154
job=$(mktemp -d "$state/runs/$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")
job_id=$(basename "$job")
mkdir -p "$state/input/$job_id"
input="$state/input/$job_id"
cp "$installer" "$input/candidate.exe"
candidate_hash=$(sha256sum "$input/candidate.exe" | cut -d' ' -f1)
previous_hash=
if [[ -n "$previous" ]]; then
    cp "$previous" "$input/previous.exe"
    previous_hash=$(sha256sum "$input/previous.exe" | cut -d' ' -f1)
fi
printf '{"candidateSha256":"%s","previousSha256":"%s","requestedAt":"%s"}\n' "$candidate_hash" "$previous_hash" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$input/request.json"
printf '%s\n' "$installer" > "$job/installer-path.txt"
printf '%s\n' "$previous" > "$job/previous-path.txt"
cp "$input/request.json" "$job/request.json"
(cd "$here" && sha256sum guest/*.ps1 oem/agent.ps1) > "$job/harness.sha256"
printf 'Acceptance evidence: %s\n' "$job"
exec > >(tee "$job/run.log") 2>&1
export_evidence() {
    # Export only this run's test evidence, never profiles, passwords, or VM state.
    transfer -r "Controller@127.0.0.1:C:/Acceptance/jobs/$job_id/evidence" "$job/"
}
failure_evidence() {
    local code=$?
    trap - EXIT
    if (( code != 0 )); then
        printf '{"status":"failed","error":"Host orchestration failed; inspect run.log","exitCode":%s}\n' "$code" > "$job/host-failure.json"
        if declare -F control >/dev/null; then
            control Stop || echo 'Could not stop the guest test task; inspect the retained VM.'
        fi
        export_evidence || echo 'Guest evidence unavailable; host log retained.'
        echo "Acceptance failed. Evidence: $job"
        echo "Tester preserved for inspection: http://127.0.0.1:$viewer"
    fi
    exit "$code"
}
trap failure_evidence EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
remote "powershell.exe -NoProfile -NonInteractive -Command \"New-Item -ItemType Directory -Force incoming/$job_id | Out-Null\""
transfer "$input/"* "$here/guest/"*.ps1 "$here/oem/agent.ps1" "Controller@127.0.0.1:incoming/$job_id/"
control() { remote "powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File incoming/$job_id/control.ps1 -Job $job_id -Action $1"; }
control Stage
control Start
rebooted=false
deadline=$((SECONDS + 1800))
status=running
while (( SECONDS < deadline )); do
    status=$(control Status | tr -d '\r')
    case "$status" in
        passed|failed) break ;;
        awaiting-reboot)
            if [[ "$rebooted" == false ]]; then
                echo 'Installation passed; rebooting to verify a fresh user PATH.'
                remote 'shutdown.exe /r /t 2' >/dev/null
                sleep 15
                wait_ssh 600
                wait_ready
                control Start
                rebooted=true
            fi ;;
        running) ;;
        *) printf 'Unexpected tester status: %s\n' "$status"; status=failed; break ;;
    esac
    sleep 5
done
if [[ "$status" != passed && "$status" != failed ]]; then
    control Stop || echo 'Could not stop the timed-out guest task.'
    printf '{"status":"failed","error":"Acceptance run timed out"}\n' > "$job/host-failure.json"
    status=failed
fi
export_evidence
echo "Acceptance result: $status"
echo "Evidence: $job/evidence/results.json"
if [[ "$status" != passed ]]; then
    trap - EXIT
    echo "Tester preserved for inspection: http://127.0.0.1:$viewer"
    exit 1
fi
compose stop
trap - EXIT
echo 'Tester stopped. Manual start will resume its final state; verify restores the baseline.'
