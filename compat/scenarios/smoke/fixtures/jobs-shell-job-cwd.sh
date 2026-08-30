#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    case "$ZZ_SMOKE_ZZ_BIN" in
    /*) zz_bin=$ZZ_SMOKE_ZZ_BIN ;;
    *) zz_bin="$(cd "$(dirname "$ZZ_SMOKE_ZZ_BIN")" && pwd)/$(basename "$ZZ_SMOKE_ZZ_BIN")" ;;
    esac
    main_client() {
        "$zz_bin" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
    cold_socket="/tmp/zzsjc-$$.sock"
    cold_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$zz_bin" --socket "$cold_socket" "$@"
    }
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
    cold_label="zzsjc-$$"
    cold_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$cold_label" "$@"
    }
fi

work="$HOME/jobs-shell-job-cwd-work"
command_cwd="$work/command-$side"
target_cwd="$work/target-$side"
startup_cwd="$work/startup-$side"
future_cwd="$work/future-$side"
failures="$work/failures-$side"
mkdir -p "$command_cwd" "$target_cwd" "$startup_cwd"
command_cwd="$(cd "$command_cwd" && pwd -P)"
target_cwd="$(cd "$target_cwd" && pwd -P)"
startup_cwd="$(cd "$startup_cwd" && pwd -P)"
: >"$failures"
failed=0
check_count=0
cold_started=0

record_failure() {
    failed=1
    printf '%s\n' "$1" >>"$failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1"
    fi
}

wait_for_file() {
    marker=$1
    value=""
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ -f "$marker" ]; then
            value="$(sed -n '1p' "$marker")"
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    printf '%s' "$value"
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    main_client set-environment -gu JOB_SHELL_EXPECTED_COMMAND_CWD >/dev/null 2>&1
    main_client set-environment -gu JOB_SHELL_IF_RESULT >/dev/null 2>&1
    main_client set-environment -gu JOB_SHELL_DELAY_MARKER >/dev/null 2>&1
    main_client set-environment -gu JOB_SHELL_FUTURE_MARKER >/dev/null 2>&1
    main_client kill-session -t =jobs-shell-cwd-target >/dev/null 2>&1
    if [ "$cold_started" -eq 1 ]; then
        cold_client kill-server >/dev/null 2>&1
    fi
    if [ "$side" = zz ]; then
        case "$cold_socket" in
        /tmp/zzsjc-[0-9]*.sock)
            rm -f -- "$cold_socket" "${cold_socket}.identity" "${cold_socket}.lock"
            ;;
        esac
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

main_client new-session -d -s jobs-shell-cwd-target -c "$target_cwd" -- sleep 30
main_client set-environment -g JOB_SHELL_EXPECTED_COMMAND_CWD "$command_cwd"

command_run_target_marker="$work/command-run-target-$side"
rm -f -- "$command_run_target_marker"
command_run_target_command="pwd -P > \"$command_run_target_marker\""
(
    cd "$command_cwd"
    main_client run-shell -t =jobs-shell-cwd-target:0.0 "$command_run_target_command"
)
command_run_target="$(wait_for_file "$command_run_target_marker")"
check_equal command-run-target "$command_cwd" "$command_run_target"

command_run_missing_marker="$work/command-run-missing-$side"
rm -f -- "$command_run_missing_marker"
command_run_missing_command="pwd -P > \"$command_run_missing_marker\""
(
    cd "$command_cwd"
    main_client run-shell -t =jobs-shell-cwd-missing:0.0 "$command_run_missing_command"
)
command_run_missing="$(wait_for_file "$command_run_missing_marker")"
check_equal command-run-missing "$command_cwd" "$command_run_missing"

main_client set-environment -gu JOB_SHELL_IF_RESULT
(
    cd "$command_cwd"
    main_client if-shell -t =jobs-shell-cwd-target:0.0 \
        'test "$(pwd -P)" = "$JOB_SHELL_EXPECTED_COMMAND_CWD"' \
        'set-environment -g JOB_SHELL_IF_RESULT target-clean' \
        'set-environment -g JOB_SHELL_IF_RESULT broken'
)
command_if_target="$(main_client show-environment -g JOB_SHELL_IF_RESULT 2>/dev/null || true)"
check_equal command-if-target JOB_SHELL_IF_RESULT=target-clean "$command_if_target"

main_client set-environment -gu JOB_SHELL_IF_RESULT
(
    cd "$command_cwd"
    main_client if-shell -t =jobs-shell-cwd-missing:0.0 \
        'test "$(pwd -P)" = "$JOB_SHELL_EXPECTED_COMMAND_CWD"' \
        'set-environment -g JOB_SHELL_IF_RESULT missing-clean' \
        'set-environment -g JOB_SHELL_IF_RESULT broken'
)
command_if_missing="$(main_client show-environment -g JOB_SHELL_IF_RESULT 2>/dev/null || true)"
check_equal command-if-missing JOB_SHELL_IF_RESULT=missing-clean "$command_if_missing"

delay_marker="$work/delay-$side"
rm -f -- "$delay_marker"
main_client set-environment -g JOB_SHELL_DELAY_MARKER "$delay_marker"
(
    cd "$command_cwd"
    main_client run-shell -b -d 0.3 'pwd -P > "$JOB_SHELL_DELAY_MARKER"'
)
delay_value="$(wait_for_file "$delay_marker")"
check_equal positive-delay-client "$command_cwd" "$delay_value"

future_marker="$work/future-marker-$side"
rm -f -- "$future_marker"
rmdir -- "$future_cwd" >/dev/null 2>&1 || true
main_client set-environment -g JOB_SHELL_FUTURE_MARKER "$future_marker"
main_client run-shell -b -d 0.3 -c "$future_cwd" 'pwd -P > "$JOB_SHELL_FUTURE_MARKER"'
mkdir "$future_cwd"
future_cwd="$(cd "$future_cwd" && pwd -P)"
future_value="$(wait_for_file "$future_marker")"
check_equal positive-delay-explicit "$future_cwd" "$future_value"

cold_config="$work/cold-$side.conf"
startup_marker="$work/startup-marker-$side"
rm -f -- "$startup_marker"
{
    printf 'run-shell '\''pwd -P > "%s"'\''\n' "$startup_marker"
    printf 'if-shell '\''test "$(pwd -P)" = "%s"'\'' '\''set-environment -g JOB_SHELL_STARTUP_IF clean'\'' '\''set-environment -g JOB_SHELL_STARTUP_IF broken'\''\n' "$startup_cwd"
} >"$cold_config"

if [ "$side" = zz ]; then
    cold_started=1
    if ! (
        cd "$startup_cwd"
        exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$zz_bin" --socket "$cold_socket" -f "$cold_config" \
            new-session -d -s jobs-shell-cwd-cold
    ) >"$work/cold-$side.out" 2>"$work/cold-$side.err"; then
        record_failure cold-start
    fi
else
    cold_started=1
    if ! (
        cd "$startup_cwd"
        exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$cold_label" -f "$cold_config" \
            new-session -d -s jobs-shell-cwd-cold
    ) >"$work/cold-$side.out" 2>"$work/cold-$side.err"; then
        record_failure cold-start
    fi
fi

startup_run="$(wait_for_file "$startup_marker")"
check_equal startup-run "$startup_cwd" "$startup_run"
startup_if="$(cold_client show-environment -g JOB_SHELL_STARTUP_IF 2>/dev/null || true)"
check_equal startup-if JOB_SHELL_STARTUP_IF=clean "$startup_if"

if [ "$check_count" -ne 8 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g JOBS_SHELL_JOB_CWD clean:8
else
    sed "s/^/jobs-shell-job-cwd-$side: /" "$failures"
fi
