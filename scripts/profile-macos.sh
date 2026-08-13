#!/usr/bin/env bash
set -euo pipefail

usage() {
    printf '%s\n' \
        "usage: scripts/profile-macos.sh <cpu|system|metal|diagnostics> [target] [duration]" \
        "" \
        "targets:" \
        "  cpu:    gui (default), daemon, or all" \
        "  system: all" \
        "  metal:  gui" \
        "  diagnostics: gui" \
        "" \
        "duration uses xctrace syntax (for example 500ms, 20s, or 2m)." \
        "Set ZZ_PROFILE_WARMUP_SECONDS to change the default 5-second warmup."
}

fail() {
    printf 'zz profiling: %s\n' "$*" >&2
    exit 2
}

if [[ $# -lt 1 || $# -gt 3 ]]; then
    usage >&2
    exit 2
fi

PROFILE_MODE=$1
PROFILE_TARGET=${2:-gui}
PROFILE_DURATION=${3:-20s}
PROFILE_WARMUP_SECONDS=${ZZ_PROFILE_WARMUP_SECONDS:-5}

case "$PROFILE_MODE" in
    cpu)
        case "$PROFILE_TARGET" in
            gui | daemon | all) ;;
            *) fail "CPU target must be gui, daemon, or all" ;;
        esac
        ;;
    system)
        [[ "$PROFILE_TARGET" == "all" ]] ||
            fail "system captures require the all target"
        ;;
    metal)
        [[ "$PROFILE_TARGET" == "gui" ]] ||
            fail "metal captures require the gui target"
        ;;
    diagnostics)
        [[ "$PROFILE_TARGET" == "gui" ]] ||
            fail "diagnostic captures require the gui target"
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac

[[ "$PROFILE_DURATION" =~ ^[1-9][0-9]*(ms|s|m|h)$ ]] ||
    fail "invalid duration $PROFILE_DURATION"
[[ "$PROFILE_WARMUP_SECONDS" =~ ^[0-9]+([.][0-9]+)?$ ]] ||
    fail "ZZ_PROFILE_WARMUP_SECONDS must be a non-negative number"
[[ "$(uname -s)" == "Darwin" ]] || fail "macOS is required"

command -v xcrun >/dev/null || fail "xcrun is required"
command -v dwarfdump >/dev/null || fail "dwarfdump is required"
command -v pgrep >/dev/null || fail "pgrep is required"

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
PROFILE_APP=${ZZ_PROFILE_APP:-"$REPO_ROOT/dist/zz-profile/zz.app"}
APP_BINARY="$PROFILE_APP/Contents/MacOS/zz"
HELPER_BINARY="$PROFILE_APP/Contents/Frameworks/zz Helper.app/Contents/MacOS/zz Helper"
SYMBOLS_DIR="$(dirname "$PROFILE_APP")/symbols"

[[ -x "$APP_BINARY" ]] ||
    fail "profiling bundle is missing; run 'just profile-build mac' first"
[[ -x "$HELPER_BINARY" ]] ||
    fail "profiling helper is missing; run 'just profile-build mac' first"
[[ -d "$SYMBOLS_DIR/zz.dSYM" && -d "$SYMBOLS_DIR/zz_helper.dSYM" ]] ||
    fail "matching profiling dSYMs are missing; run 'just profile-build mac' first"

PROFILE_EXISTING_PIDS=()
while IFS= read -r candidate_pid; do
    [[ "$candidate_pid" =~ ^[1-9][0-9]*$ ]] || continue
    candidate_command=$(ps -p "$candidate_pid" -o command= 2>/dev/null || true)
    candidate_command=${candidate_command#"${candidate_command%%[![:space:]]*}"}
    case "$candidate_command" in
        "$APP_BINARY" | "$APP_BINARY "*)
            PROFILE_EXISTING_PIDS+=("$candidate_pid")
            ;;
    esac
done < <(pgrep -x zz 2>/dev/null || true)

if [[ ${#PROFILE_EXISTING_PIDS[@]} -gt 0 ]]; then
    PROFILE_EXISTING_PID_LIST=$(IFS=,; printf '%s' "${PROFILE_EXISTING_PIDS[*]}")
    fail "profiling bundle is already running (pid $PROFILE_EXISTING_PID_LIST); quit it before recording so the captured and automated window cannot be confused"
fi

PROFILE_OUTPUT_ROOT=${ZZ_PROFILE_OUTPUT_DIR:-"$REPO_ROOT/target/profiles"}
mkdir -p "$PROFILE_OUTPUT_ROOT"
PROFILE_TIMESTAMP=$(date -u '+%Y%m%dT%H%M%SZ')
PROFILE_RUN_DIR=$(mktemp -d "$PROFILE_OUTPUT_ROOT/$PROFILE_TIMESTAMP-$PROFILE_MODE-$PROFILE_TARGET.XXXXXX")

PROFILE_TEMP_ROOT=${TMPDIR:-/tmp}
PROFILE_TEMP_ROOT=${PROFILE_TEMP_ROOT%/}
PROFILE_RUNTIME_DIR=$(mktemp -d "$PROFILE_TEMP_ROOT/zz-profile.XXXXXX")
PROFILE_SOCKET="$PROFILE_RUNTIME_DIR/zz.sock"
PROFILE_IDENTITY="$PROFILE_SOCKET.identity"
PROFILE_TRACE="$PROFILE_RUN_DIR/$PROFILE_MODE-$PROFILE_TARGET.trace"

GUI_PID=
DAEMON_PID=

stop_owned_processes() {
    local exit_status=$?
    trap - EXIT INT TERM

    if [[ -n "$GUI_PID" ]] && kill -0 "$GUI_PID" 2>/dev/null; then
        kill -TERM "$GUI_PID" 2>/dev/null || true
        local attempt=0
        while kill -0 "$GUI_PID" 2>/dev/null && [[ $attempt -lt 60 ]]; do
            sleep 0.05
            attempt=$((attempt + 1))
        done
        if kill -0 "$GUI_PID" 2>/dev/null; then
            kill -KILL "$GUI_PID" 2>/dev/null || true
        fi
        wait "$GUI_PID" 2>/dev/null || true
    fi

    if [[ -n "$DAEMON_PID" ]] && kill -0 "$DAEMON_PID" 2>/dev/null; then
        "$APP_BINARY" --socket "$PROFILE_SOCKET" kill-server >/dev/null 2>&1 || true
        local daemon_attempt=0
        while kill -0 "$DAEMON_PID" 2>/dev/null && [[ $daemon_attempt -lt 60 ]]; do
            sleep 0.05
            daemon_attempt=$((daemon_attempt + 1))
        done
        if kill -0 "$DAEMON_PID" 2>/dev/null; then
            kill -TERM "$DAEMON_PID" 2>/dev/null || true
        fi
    fi

    if [[ -d "$PROFILE_RUNTIME_DIR" ]]; then
        case "$PROFILE_RUNTIME_DIR" in
            "$PROFILE_TEMP_ROOT"/zz-profile.*)
                rm -rf -- "$PROFILE_RUNTIME_DIR"
                ;;
            *)
                printf 'zz profiling: refusing to remove unexpected runtime path %s\n' \
                    "$PROFILE_RUNTIME_DIR" >&2
                ;;
        esac
    fi
    exit "$exit_status"
}

trap 'exit 130' INT
trap 'exit 143' TERM
trap stop_owned_processes EXIT

uuid_list() {
    dwarfdump --uuid "$1" | awk '/^UUID: / { print $2 }' | sort -u
}

verify_debug_symbols() {
    local binary=$1
    local symbols=$2
    local label=$3
    local binary_uuids="$PROFILE_RUNTIME_DIR/$label.binary-uuids"
    local symbol_uuids="$PROFILE_RUNTIME_DIR/$label.symbol-uuids"

    uuid_list "$binary" >"$binary_uuids"
    uuid_list "$symbols" >"$symbol_uuids"
    [[ -s "$binary_uuids" ]] || fail "could not read $label binary UUID"
    cmp -s "$binary_uuids" "$symbol_uuids" ||
        fail "$label binary and dSYM UUIDs do not match"
}

capture_process_tree() {
    local destination=$1
    local pids="$PROFILE_RUNTIME_DIR/process-pids"
    local parents="$PROFILE_RUNTIME_DIR/process-parents"
    local depth=0

    printf '%s\n' "$GUI_PID" "$DAEMON_PID" | awk 'NF' | sort -nu >"$pids"
    while [[ $depth -lt 6 ]]; do
        cp "$pids" "$parents"
        while IFS= read -r parent_pid; do
            pgrep -P "$parent_pid" >>"$pids" 2>/dev/null || true
        done <"$parents"
        sort -nu -o "$pids" "$pids"
        depth=$((depth + 1))
    done

    local pid_csv
    pid_csv=$(paste -sd, "$pids")
    if [[ -n "$pid_csv" ]]; then
        ps -p "$pid_csv" -o pid=,ppid=,pgid=,%cpu=,%mem=,etime=,comm=,args= \
            >"$destination" 2>/dev/null || true
    fi
}

verify_debug_symbols "$APP_BINARY" "$SYMBOLS_DIR/zz.dSYM" app
verify_debug_symbols "$HELPER_BINARY" "$SYMBOLS_DIR/zz_helper.dSYM" helper

mkdir -p "$PROFILE_RUN_DIR/logs"
export ZZ_LOG_DIR="$PROFILE_RUN_DIR/logs"
if [[ "$PROFILE_MODE" == "diagnostics" ]]; then
    export RUST_LOG=${ZZ_PROFILE_LOG_FILTER:-"zz::diagnostics::terminal_render=trace"}
fi
"$APP_BINARY" --socket "$PROFILE_SOCKET" \
    >"$PROFILE_RUN_DIR/app.stdout.log" \
    2>"$PROFILE_RUN_DIR/app.stderr.log" &
GUI_PID=$!

printf 'zz profiling: launched GUI pid %s with isolated socket %s\n' \
    "$GUI_PID" "$PROFILE_SOCKET"

identity_attempt=0
while [[ ! -s "$PROFILE_IDENTITY" && $identity_attempt -lt 200 ]]; do
    if ! kill -0 "$GUI_PID" 2>/dev/null; then
        printf 'zz profiling: GUI exited before the daemon became ready\n' >&2
        tail -n 40 "$PROFILE_RUN_DIR/app.stderr.log" >&2 || true
        exit 1
    fi
    sleep 0.05
    identity_attempt=$((identity_attempt + 1))
done
[[ -s "$PROFILE_IDENTITY" ]] ||
    fail "isolated daemon did not become ready within 10 seconds"

DAEMON_PID=$(awk -F= '$1 == "pid" { print $2 }' "$PROFILE_IDENTITY")
[[ "$DAEMON_PID" =~ ^[1-9][0-9]*$ ]] ||
    fail "daemon identity did not contain a valid PID"
kill -0 "$DAEMON_PID" 2>/dev/null ||
    fail "isolated daemon pid $DAEMON_PID is not running"

{
    printf 'run_id=%s-%s-%s\n' "$PROFILE_TIMESTAMP" "$PROFILE_MODE" "$PROFILE_TARGET"
    printf 'commit=%s\n' "$(git -C "$REPO_ROOT" rev-parse HEAD)"
    printf 'mode=%s\n' "$PROFILE_MODE"
    printf 'target=%s\n' "$PROFILE_TARGET"
    printf 'duration=%s\n' "$PROFILE_DURATION"
    printf 'warmup_seconds=%s\n' "$PROFILE_WARMUP_SECONDS"
    printf 'app=%s\n' "$PROFILE_APP"
    printf 'socket=%s\n' "$PROFILE_SOCKET"
    printf 'gui_pid=%s\n' "$GUI_PID"
    printf 'daemon_pid=%s\n' "$DAEMON_PID"
    printf 'host=%s\n' "$(sw_vers -productVersion)"
    printf 'architecture=%s\n' "$(uname -m)"
    printf 'xctrace=%s\n' "$(xcrun xctrace version)"
    if [[ "$PROFILE_MODE" == "diagnostics" ]]; then
        printf 'log_filter=%s\n' "$RUST_LOG"
    fi
    for variable in \
        ZZ_BROWSER_FPS \
        ZZ_BROWSER_GPU \
        ZZ_BROWSER_SHARED_TEXTURE \
        ZZ_BROWSER_EXTERNAL_BEGIN_FRAME \
        ZZ_BROWSER_BF_ADAPTIVE
    do
        if [[ -n "${!variable-}" ]]; then
            printf '%s=%s\n' "$variable" "${!variable}"
        fi
    done
} >"$PROFILE_RUN_DIR/metadata.txt"
git -C "$REPO_ROOT" status --short >"$PROFILE_RUN_DIR/git-status.txt"
uuid_list "$APP_BINARY" >"$PROFILE_RUN_DIR/app-uuids.txt"
uuid_list "$HELPER_BINARY" >"$PROFILE_RUN_DIR/helper-uuids.txt"
capture_process_tree "$PROFILE_RUN_DIR/processes-before.txt"

if [[ "$PROFILE_WARMUP_SECONDS" != "0" ]]; then
    printf 'zz profiling: capture starts in %s seconds; prepare the scenario now\n' \
        "$PROFILE_WARMUP_SECONDS"
    sleep "$PROFILE_WARMUP_SECONDS"
fi

capture_process_tree "$PROFILE_RUN_DIR/processes-capture.txt"

case "$PROFILE_MODE" in
    cpu)
        PROFILE_TEMPLATE="Time Profiler"
        case "$PROFILE_TARGET" in
            gui)
                PROFILE_TARGET_ARGUMENTS=(--attach "$GUI_PID")
                ;;
            daemon)
                PROFILE_TARGET_ARGUMENTS=(--attach "$DAEMON_PID")
                ;;
            all)
                PROFILE_TARGET_ARGUMENTS=(--all-processes)
                ;;
        esac
        ;;
    system)
        PROFILE_TEMPLATE="System Trace"
        PROFILE_TARGET_ARGUMENTS=(--all-processes)
        ;;
    metal)
        PROFILE_TEMPLATE="Metal System Trace"
        PROFILE_TARGET_ARGUMENTS=(--attach "$GUI_PID")
        ;;
    diagnostics)
        PROFILE_TEMPLATE=
        PROFILE_TARGET_ARGUMENTS=()
        ;;
esac

if [[ "$PROFILE_MODE" == "diagnostics" ]]; then
    case "$PROFILE_DURATION" in
        *ms)
            duration_milliseconds=${PROFILE_DURATION%ms}
            duration_sleep_seconds=$((duration_milliseconds / 1000))
            printf -v duration_sleep_fraction '%03d' "$((duration_milliseconds % 1000))"
            duration_sleep="$duration_sleep_seconds.$duration_sleep_fraction"
            ;;
        *s)
            duration_sleep=${PROFILE_DURATION%s}
            ;;
        *m)
            duration_sleep=$((10#${PROFILE_DURATION%m} * 60))
            ;;
        *h)
            duration_sleep=$((10#${PROFILE_DURATION%h} * 3600))
            ;;
    esac
    printf 'zz profiling: collecting diagnostic logs for %s into %s\n' \
        "$PROFILE_DURATION" "$PROFILE_RUN_DIR"
    sleep "$duration_sleep"
else
    printf 'zz profiling: recording %s for %s into %s\n' \
        "$PROFILE_TEMPLATE" "$PROFILE_DURATION" "$PROFILE_TRACE"
    xcrun xctrace record \
        --template "$PROFILE_TEMPLATE" \
        "${PROFILE_TARGET_ARGUMENTS[@]}" \
        --time-limit "$PROFILE_DURATION" \
        --run-name "$PROFILE_MODE-$PROFILE_TARGET" \
        --output "$PROFILE_TRACE"
fi

capture_process_tree "$PROFILE_RUN_DIR/processes-after.txt"
printf 'zz profiling: capture complete: %s\n' "$PROFILE_RUN_DIR"
