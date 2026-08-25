#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    control_side=zz
else
    control_side=tmux
fi
control_dir="$HOME/source-file-control-$control_side"
mkdir -p "$control_dir"
printf '' >"$control_dir/hit.conf"
printf '%s\n' 'source-file zz-control-nested-one-missing.conf zz-control-nested-two-missing.conf' >"$control_dir/nested.conf"
printf '%s\n' 'wibble' >"$control_dir/invalid.conf"
depth_dir="$control_dir/depth"
mkdir -p "$depth_dir"
printf '%s\n' 'set-option -g @leaf yes' >"$depth_dir/leaf.conf"
depth_level=1
while [ "$depth_level" -le 50 ]; do
    {
        printf 'set-option -g @depth %s\n' "$depth_level"
        if [ "$depth_level" -lt 50 ]; then
            printf 'source-file %s/f%s.conf\n' "$depth_dir" "$((depth_level + 1))"
        else
            printf 'source-file %s/leaf.conf\n' "$depth_dir"
            printf 'source-file -q %s/leaf.conf\n' "$depth_dir"
        fi
        printf 'set-option -g @after%s yes\n' "$depth_level"
    } >"$depth_dir/f$depth_level.conf"
    depth_level=$((depth_level + 1))
done

probe_daemon_pid=''
control_pid=''
probe_socket=''
probe_label=''

wait_for_process() {
    wait_pid=$1
    wait_limit=$2
    wait_attempt=0
    while kill -0 "$wait_pid" 2>/dev/null && [ "$wait_attempt" -lt "$wait_limit" ]; do
        wait_attempt=$((wait_attempt + 1))
        sleep 0.05
    done
    if kill -0 "$wait_pid" 2>/dev/null; then
        kill -TERM "$wait_pid" 2>/dev/null || true
        wait_attempt=0
        while kill -0 "$wait_pid" 2>/dev/null && [ "$wait_attempt" -lt 20 ]; do
            wait_attempt=$((wait_attempt + 1))
            sleep 0.05
        done
    fi
    if kill -0 "$wait_pid" 2>/dev/null; then
        kill -KILL "$wait_pid" 2>/dev/null || true
        wait_attempt=0
        while kill -0 "$wait_pid" 2>/dev/null && [ "$wait_attempt" -lt 20 ]; do
            wait_attempt=$((wait_attempt + 1))
            sleep 0.05
        done
    fi
    if kill -0 "$wait_pid" 2>/dev/null; then
        return 124
    fi
    wait "$wait_pid"
}

cleanup_probe() {
    cleanup_status=$?
    set +e
    exec 3>&-
    shutdown_pid=''
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        if [ -n "$probe_socket" ]; then
            "$ZZ_SMOKE_ZZ_BIN" --socket "$probe_socket" kill-server >/dev/null 2>&1 &
            shutdown_pid=$!
        fi
    elif [ -n "$probe_label" ]; then
        "$ZZ_SMOKE_TMUX_BIN" -L "$probe_label" kill-server >/dev/null 2>&1 &
        shutdown_pid=$!
    fi
    if [ -n "$shutdown_pid" ]; then
        wait_for_process "$shutdown_pid" 40 >/dev/null 2>&1
    fi
    if [ -n "$control_pid" ]; then
        wait_for_process "$control_pid" 100 >/dev/null 2>&1
    fi
    if [ -n "$probe_daemon_pid" ]; then
        wait_for_process "$probe_daemon_pid" 100 >/dev/null 2>&1
    fi
    case "$probe_socket" in
    /tmp/zzsfc-[0-9]*.sock) rm -f -- "$probe_socket" ;;
    esac
    return "$cleanup_status"
}
trap cleanup_probe EXIT
trap 'exit 1' HUP INT TERM

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    probe_socket="/tmp/zzsfc-$$.sock"
    "$ZZ_SMOKE_ZZ_BIN" --socket "$probe_socket" -f /dev/null daemon \
        >"$control_dir/daemon.out" 2>"$control_dir/daemon.err" &
    probe_daemon_pid=$!
    attempt=0
    until [ -S "$probe_socket" ]; do
        attempt=$((attempt + 1))
        if [ "$attempt" -ge 200 ] || ! kill -0 "$probe_daemon_pid" 2>/dev/null; then
            sed -n '1,120p' "$control_dir/daemon.err" >&2
            exit 1
        fi
        sleep 0.05
    done
    "$ZZ_SMOKE_ZZ_BIN" --socket "$probe_socket" new-session -d -s w
else
    probe_label="zzsfc-$$"
    "$ZZ_SMOKE_TMUX_BIN" -L "$probe_label" -f /dev/null new-session -d -s w
fi

control_client() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$probe_socket" \
            -C attach-session -t w
    else
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$probe_label" \
            -C attach-session -t w
    fi
}

main_client() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    else
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    fi
}

probe_command() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        "$ZZ_SMOKE_ZZ_BIN" --socket "$probe_socket" "$@"
    else
        "$ZZ_SMOKE_TMUX_BIN" -L "$probe_label" "$@"
    fi
}

raw="$control_dir/control.raw"
errors="$control_dir/control.err"
input="$control_dir/control.in"
: >"$raw"
: >"$errors"
mkfifo "$input"
control_client <"$input" >"$raw" 2>"$errors" &
control_pid=$!
exec 3>"$input"
printf "source-file '%s' '%s' ; display-message -p DIRECT_SHOULD_NOT_RUN\n" \
    "$control_dir/direct-one-missing.conf" "$control_dir/direct-two-missing.conf" >&3
printf "source-file '%s' '%s' ; display-message -p PARTIAL_CONT\n" \
    "$control_dir/hit.conf" "$control_dir/partial-missing.conf" >&3
printf "source-file '%s' ; display-message -p NESTED_CONT\n" \
    "$control_dir/nested.conf" >&3
printf "source-file '%s' ; display-message -p INVALID_CONT\n" \
    "$control_dir/invalid.conf" >&3
printf '%s\n' 'run-shell "exit 3" ; display-message -p RUN_CONT' >&3
printf '%s\n' 'display-message -p CONTROL_DONE' >&3

attempt=0
until awk '
    /^CONTROL_DONE$/ { done_payload = 1; next }
    done_payload && /^%end [0-9]+ [0-9]+ [0-9]+$/ { complete = 1 }
    END { exit !complete }
' "$raw"; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 200 ] || ! kill -0 "$control_pid" 2>/dev/null; then
        sed -n '1,120p' "$errors" >&2
        exit 1
    fi
    sleep 0.05
done

printf '%s\n' 'detach-client' >&3
exec 3>&-
if wait_for_process "$control_pid" 100; then
    control_status=0
else
    control_status=$?
fi
control_pid=''
if [ "$control_status" -ne 0 ]; then
    sed -n '1,120p' "$errors" >&2
    exit "$control_status"
fi
if [ -s "$errors" ]; then
    sed -n '1,120p' "$errors" >&2
    exit 1
fi

transcript="$(
    sed "s|$control_dir/||g" "$raw" | awk '
        /^%begin [0-9]+ [0-9]+ [0-9]+$/ {
            if (!started && $4 != 1) next
            active = 1
            started = 1
            print "%begin"
            next
        }
        /^%end [0-9]+ [0-9]+ [0-9]+$/ {
            if (active || started) print "%end"
            active = 0
            next
        }
        /^%error [0-9]+ [0-9]+ [0-9]+$/ {
            if (active || started) print "%error"
            active = 0
            next
        }
        /^%config-error / {
            print
            next
        }
        /^'\''exit 3'\'' returned 3$/ {
            next
        }
        /^%exit$/ {
            next
        }
        active {
            print
            if ($0 == "CONTROL_DONE") done_payload = 1
            next
        }
        started && !/^%/ {
            print
        }
    ' | tr '\n' '~'
)"

main_client set-environment -g SOURCE_FILE_CONTROL "$transcript"

depth_raw="$control_dir/depth.raw"
depth_errors="$control_dir/depth.err"
depth_input="$control_dir/depth.in"
: >"$depth_raw"
: >"$depth_errors"
mkfifo "$depth_input"
control_client <"$depth_input" >"$depth_raw" 2>"$depth_errors" &
control_pid=$!
exec 3>"$depth_input"
printf "source-file '%s' ; display-message -p DEPTH_CONT\n" \
    "$depth_dir/f1.conf" >&3
printf '%s\n' 'display-message -p DEPTH_DONE' >&3

attempt=0
until awk '
    /^DEPTH_DONE$/ { done_payload = 1; next }
    done_payload && /^%end [0-9]+ [0-9]+ [0-9]+$/ { complete = 1 }
    END { exit !complete }
' "$depth_raw"; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 400 ] || ! kill -0 "$control_pid" 2>/dev/null; then
        sed -n '1,120p' "$depth_errors" >&2
        exit 1
    fi
    sleep 0.05
done

printf '%s\n' 'detach-client' >&3
exec 3>&-
if wait_for_process "$control_pid" 100; then
    depth_status=0
else
    depth_status=$?
fi
control_pid=''
if [ "$depth_status" -ne 0 ] || [ -s "$depth_errors" ]; then
    sed -n '1,120p' "$depth_errors" >&2
    exit 1
fi

depth_hits="$(grep -c '^too many nested files$' "$depth_raw" || true)"
depth_cont="$(grep -c '^DEPTH_CONT$' "$depth_raw" || true)"
main_client set-environment -g SOURCE_FILE_CONTROL_DEPTH \
    "rc=$depth_status hits=$depth_hits cont=$depth_cont depth=$(probe_command show-options -gqv @depth) last=$(probe_command show-options -gqv @after50) leaf=$(probe_command show-options -gqv @leaf)"
exit 0
