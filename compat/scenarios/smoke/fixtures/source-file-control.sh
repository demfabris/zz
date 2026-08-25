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
printf '%s\n' 'set-option -g @control_verbose loaded' >"$control_dir/verbose.conf"
printf '%s\n' \
    'kill-session -t missing-runtime' \
    'set-option -g nonexistent-option value' \
    'set-option -g @runtime_after yes' >"$control_dir/runtime.conf"
printf '%s\n' 'kill-session -t status-sourced-runtime' >"$control_dir/status-runtime.conf"
printf '%s\n' 'source-file status-nested-missing.conf' >"$control_dir/status-source.conf"
queue_dir="$control_dir/queue"
mkdir -p "$queue_dir"
printf '%s\n' 'display-message -p CONTROL_QUEUE_LEAF' >"$queue_dir/leaf.conf"
printf "source-file middle-before-missing.conf '%s' middle-after-missing.conf\n" \
    "$queue_dir/leaf.conf" >"$queue_dir/middle.conf"
printf "source-file root-before-missing.conf '%s' root-after-missing.conf\n" \
    "$queue_dir/middle.conf" >"$queue_dir/root.conf"
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

wait_for_marker() {
    marker_path=$1
    marker_pid=$2
    marker_label=$3
    marker_attempt=0
    until [ -e "$marker_path" ]; do
        marker_attempt=$((marker_attempt + 1))
        if [ "$marker_attempt" -ge 200 ] || ! kill -0 "$marker_pid" 2>/dev/null; then
            printf 'control marker not reached: %s\n' "$marker_label" >&2
            return 1
        fi
        sleep 0.05
    done
}

wait_for_control_output_marker() {
    marker_path=$1
    marker_value=$2
    marker_pid=$3
    marker_label=$4
    marker_attempt=0
    until awk -v target="$marker_value" '
        $0 == target { marker = 1; next }
        marker && /^%end [0-9]+ [0-9]+ [0-9]+$/ { complete = 1 }
        END { exit !complete }
    ' "$marker_path"; do
        marker_attempt=$((marker_attempt + 1))
        if [ "$marker_attempt" -ge 200 ] || ! kill -0 "$marker_pid" 2>/dev/null; then
            printf 'control output marker not reached: %s\n' "$marker_label" >&2
            return 1
        fi
        sleep 0.05
    done
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

verbose_raw="$control_dir/verbose.raw"
verbose_errors="$control_dir/verbose.err"
{
    printf "source-file -v '%s'\n" "$control_dir/verbose.conf"
    printf '%s\n' 'detach-client'
} | control_client >"$verbose_raw" 2>"$verbose_errors"
if [ ! -s "$verbose_errors" ] && \
   ! grep -Fq 'verbose.conf:' "$verbose_raw" && \
   [ "$(probe_command show-options -gqv @control_verbose)" = loaded ]; then
    main_client set-environment -g SOURCE_FILE_CONTROL_VERBOSE suppressed
else
    main_client set-environment -g SOURCE_FILE_CONTROL_VERBOSE leaked
fi

runtime_raw="$control_dir/runtime.raw"
runtime_errors="$control_dir/runtime.err"
runtime_input="$control_dir/runtime.in"
: >"$runtime_raw"
: >"$runtime_errors"
rm -f -- "$runtime_input"
mkfifo "$runtime_input"
control_client <"$runtime_input" >"$runtime_raw" 2>"$runtime_errors" &
control_pid=$!
exec 3>"$runtime_input"
printf "source-file '%s'\n" "$control_dir/runtime.conf" >&3
printf '%s\n' 'display-message -p RUNTIME_DONE' >&3
wait_for_control_output_marker "$runtime_raw" RUNTIME_DONE "$control_pid" runtime
exec 3>&-
if wait_for_process "$control_pid" 200; then
    runtime_status=0
else
    runtime_status=$?
fi
control_pid=''
runtime_kill="$(grep -c '^can'"'"'t find session: missing-runtime$' "$runtime_raw" || true)"
runtime_option="$(grep -c '^invalid option: nonexistent-option$' "$runtime_raw" || true)"
runtime_done="$(grep -c '^RUNTIME_DONE$' "$runtime_raw" || true)"
runtime_after="$(probe_command show-options -gqv @runtime_after)"
main_client set-environment -g SOURCE_FILE_CONTROL_RUNTIME \
    "rc=$runtime_status stderr=$(wc -c <"$runtime_errors" | tr -d ' ') kill=$runtime_kill option=$runtime_option done=$runtime_done after=$runtime_after"

queue_raw="$control_dir/queue.raw"
queue_errors="$control_dir/queue.err"
queue_input="$control_dir/queue.in"
: >"$queue_raw"
: >"$queue_errors"
rm -f -- "$queue_input"
mkfifo "$queue_input"
control_client <"$queue_input" >"$queue_raw" 2>"$queue_errors" &
control_pid=$!
exec 3>"$queue_input"
printf "source-file '%s'\n" "$queue_dir/root.conf" >&3
wait_for_control_output_marker "$queue_raw" CONTROL_QUEUE_LEAF "$control_pid" queue
printf '%s\n' 'detach-client' >&3
exec 3>&-
if wait_for_process "$control_pid" 200; then
    queue_status=0
else
    queue_status=$?
fi
control_pid=''
if [ "$queue_status" -gt 1 ] || [ -s "$queue_errors" ]; then
    sed -n '1,120p' "$queue_errors" >&2
    exit 1
fi
queue_transcript="$(
    sed "s|$queue_dir/||g" "$queue_raw" | awk '
        /^%begin [0-9]+ [0-9]+ 1$/ {
            active = 1
            print "%begin"
            next
        }
        /^%end [0-9]+ [0-9]+ [0-9]+$/ {
            if (active) print "%end"
            active = 0
            next
        }
        /^%error [0-9]+ [0-9]+ [0-9]+$/ {
            if (active) print "%error"
            active = 0
            next
        }
        active { print }
    ' | tr '\n' '~'
)"
main_client set-environment -g SOURCE_FILE_CONTROL_NESTED_QUEUE "$queue_transcript"

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

status_matrix=''
status_separator=''

run_control_status_case() {
    status_row=$1
    status_exit=$2
    status_command=$3
    status_pattern=$4
    status_case_dir="$control_dir/status-$status_row-$status_exit"
    status_raw="$status_case_dir.raw"
    status_errors="$status_case_dir.err"
    status_input="$status_case_dir.in"
    status_complete="$status_case_dir.complete"
    status_ready="$status_case_dir.ready"
    status_release="$status_case_dir.release"
    status_complete_payload="STATUS_COMPLETE_${status_row}_${status_exit}"
    : >"$status_raw"
    : >"$status_errors"
    rm -f -- "$status_input" "$status_complete" "$status_ready" "$status_release"
    mkfifo "$status_input"
    control_client <"$status_input" >"$status_raw" 2>"$status_errors" &
    control_pid=$!
    exec 3>"$status_input"
    printf '%s\n' "$status_command" >&3

    case "$status_exit" in
    eof | blank | detach-completed | detach-completed-eof)
        printf "display-message -p '%s'\n" "$status_complete_payload" >&3
        wait_for_control_output_marker \
            "$status_raw" "$status_complete_payload" "$control_pid" "$status_row/$status_exit"
        : >"$status_complete"
        ;;
    detach-queued-open | detach-queued-eof)
        printf "run-shell 'touch \"%s\"; while [ ! -e \"%s\" ]; do sleep 0.01; done'\n" \
            "$status_ready" "$status_release" >&3
        wait_for_marker "$status_ready" "$control_pid" "$status_row/$status_exit"
        ;;
    esac

    status_input_open=1
    case "$status_exit" in
    eof)
        exec 3>&-
        status_input_open=0
        ;;
    blank)
        printf '\n' >&3
        exec 3>&-
        status_input_open=0
        ;;
    detach-completed)
        printf '%s\n' 'detach-client' >&3
        ;;
    detach-completed-eof)
        printf '%s\n' 'detach-client' >&3
        exec 3>&-
        status_input_open=0
        ;;
    detach-queued-open)
        printf '%s\n' 'detach-client' >&3
        : >"$status_release"
        ;;
    detach-queued-eof)
        printf '%s\n' 'detach-client' >&3
        exec 3>&-
        status_input_open=0
        sleep 0.1
        : >"$status_release"
        ;;
    esac

    if wait_for_process "$control_pid" 200; then
        status_code=0
    else
        status_code=$?
    fi
    control_pid=''
    if [ "$status_input_open" -eq 1 ]; then
        exec 3>&-
    fi
    if [ "$status_code" -gt 1 ] || [ -s "$status_errors" ]; then
        sed -n '1,120p' "$status_errors" >&2
        exit 1
    fi
    status_hit="$(grep -Fxc "$status_pattern" "$status_raw" || true)"
    status_exit_count="$(grep -c '^%exit$' "$status_raw" || true)"
    status_marker=0
    if [ -e "$status_complete" ] || [ -e "$status_ready" ]; then
        status_marker=1
    fi
    status_released=0
    if [ -e "$status_release" ]; then
        status_released=1
    fi
    status_matrix="${status_matrix}${status_separator}${status_row}/${status_exit}:rc=${status_code},hit=${status_hit},marker=${status_marker},release=${status_released},exit=${status_exit_count}"
    status_separator=';'
}

for status_exit_name in eof blank detach-completed detach-queued-open detach-queued-eof; do
    run_control_status_case direct-runtime "$status_exit_name" \
        'kill-session -t status-direct-runtime' \
        "can't find session: status-direct-runtime"
    run_control_status_case sourced-runtime "$status_exit_name" \
        "source-file '$control_dir/status-runtime.conf'" \
        "can't find session: status-sourced-runtime"
    run_control_status_case sourced-command "$status_exit_name" \
        "source-file '$control_dir/status-source.conf'" \
        'No such file or directory: status-nested-missing.conf'
    run_control_status_case generic-nonzero "$status_exit_name" \
        "run-shell 'exit 3'" \
        "'exit 3' returned 3"
done
main_client set-environment -g SOURCE_FILE_CONTROL_STATUS_MATRIX "$status_matrix"

pre_ready="$control_dir/pre-failure.ready"
pre_release="$control_dir/pre-failure.release"
pre_raw="$control_dir/pre-failure.raw"
pre_errors="$control_dir/pre-failure.err"
pre_input="$control_dir/pre-failure.in"
: >"$pre_raw"
: >"$pre_errors"
rm -f -- "$pre_ready" "$pre_release" "$pre_input"
mkfifo "$pre_input"
control_client <"$pre_input" >"$pre_raw" 2>"$pre_errors" &
control_pid=$!
exec 3>"$pre_input"
printf "if-shell 'touch \"%s\"; while [ ! -e \"%s\" ]; do sleep 0.01; done; true' 'kill-session -t pre-failure-missing'\n" \
    "$pre_ready" "$pre_release" >&3
wait_for_marker "$pre_ready" "$control_pid" pre-failure
exec 3>&-
sleep 0.1
: >"$pre_release"
if wait_for_process "$control_pid" 200; then
    pre_status=0
else
    pre_status=$?
fi
control_pid=''
if [ "$pre_status" -gt 1 ] || [ -s "$pre_errors" ]; then
    sed -n '1,120p' "$pre_errors" >&2
    exit 1
fi
pre_exit="$(grep -c '^%exit$' "$pre_raw" || true)"

post_matrix_before="$status_matrix"
run_control_status_case sourced-runtime-post detach-completed-eof \
    "source-file '$control_dir/status-runtime.conf'" \
    "can't find session: status-sourced-runtime"
post_entry=${status_matrix#"$post_matrix_before;"}
main_client set-environment -g SOURCE_FILE_CONTROL_RETURN_PRECEDENCE \
    "pre:rc=$pre_status,ready=1,release=1,exit=$pre_exit;post:$post_entry"
exit 0
