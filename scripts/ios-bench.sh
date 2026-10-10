#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 3 ]] || { echo "usage: scripts/ios-bench.sh <device-udid> <app-bundle> <endpoint>" >&2; exit 2; }
udid="$1"
app="$2"
endpoint="$3"
bundle_id=dev.zz.gpui-poc
cli="${ZZ_BENCH_CLI:-zz-dev}"
session="${ZZ_BENCH_SESSION:-iphone-bench}"
cycles="${ZZ_BENCH_CYCLES:-16}"
out="${ZZ_BENCH_OUT:-$(mktemp -d "${TMPDIR:-/tmp}/zz-ios-bench.XXXXXX")}"
mkdir -p "$out"

xcrun devicectl device info lockState --device "$udid" 2>/dev/null | grep -q "passcodeRequired: false" \
    || { echo "unlock the device and keep it unlocked for the run" >&2; exit 1; }
! "$cli" has-session -t "$session" 2>/dev/null \
    || { echo "session $session already exists on $cli; set ZZ_BENCH_SESSION" >&2; exit 1; }

terminate_app() {
    local processes
    processes="$(mktemp)"
    xcrun devicectl device info processes --device "$udid" --json-output "$processes" >/dev/null 2>&1 || true
    python3 - "$processes" <<'PY' | while read -r pid; do
import json, sys
try:
    processes = json.load(open(sys.argv[1]))["result"]["runningProcesses"]
except Exception:
    processes = []
for process in processes:
    if process.get("executable", "").rstrip("/").endswith("/ZZGPUI"):
        print(process["processIdentifier"])
PY
        xcrun devicectl device process terminate --device "$udid" --pid "$pid" >/dev/null 2>&1 || true
    done
    rm -f "$processes"
}

cleanup() {
    terminate_app
    "$cli" kill-session -t "$session" >/dev/null 2>&1 || true
}
trap cleanup EXIT

"$cli" new-session -d -s "$session" -x 60 -y 40
"$cli" send-keys -t "$session" 'seq 1 4000' Enter
"$cli" split-window -h -t "$session"
"$cli" send-keys -t "$session" 'seq 1 4000' Enter
"$cli" select-pane -L -t "$session"
sleep 2

xcrun devicectl device install app --device "$udid" "$app" >/dev/null

run() {
    local kind="$1" seconds="$2" log="$out/$1.log" env_json launch elapsed=0
    shift 2
    env_json="$(python3 -c 'import json, sys; print(json.dumps(dict(arg.split("=", 1) for arg in sys.argv[1:])))' \
        "ZZ_GPUI_ENDPOINT=$endpoint" "ZZ_GPUI_SESSION=$session" ZZ_GPUI_FRAME_LOG=1 \
        ${ZZ_GPUI_GLASS:+"ZZ_GPUI_GLASS=$ZZ_GPUI_GLASS"} "$@")"
    xcrun devicectl device process launch --device "$udid" --terminate-existing --console \
        --environment-variables "$env_json" "$bundle_id" > "$log" 2>&1 &
    launch=$!
    while ((elapsed < seconds)) && ! grep -q "bench done" "$log"; do
        sleep 1
        elapsed=$((elapsed + 1))
    done
    terminate_app
    kill "$launch" 2>/dev/null || true
    wait "$launch" 2>/dev/null || true
}

kinds=" ${ZZ_BENCH_KINDS:-swipe scroll sheet idle} "
[[ "$kinds" != *" swipe "* ]] || run swipe $((15 + cycles * 14 / 10)) "ZZ_GPUI_BENCH=swipe:$cycles"
[[ "$kinds" != *" scroll "* ]] || run scroll $((15 + cycles * 3)) "ZZ_GPUI_BENCH=scroll:$cycles"
[[ "$kinds" != *" sheet "* ]] || run sheet $((15 + cycles * 3)) "ZZ_GPUI_BENCH=sheet:$cycles"
[[ "$kinds" != *" idle "* ]] || run idle 25

echo "logs: $out"
for kind in swipe scroll sheet; do
    [[ "$kinds" != *" $kind "* ]] || echo "$kind: $(grep -h "frames total" "$out/$kind.log" || echo "no result, see $out/$kind.log")"
done
[[ "$kinds" != *" idle "* ]] || echo "idle: $(grep -h "link:" "$out/idle.log" | tail -2 | tr '\n' ' ')"
