#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
    control_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" \
            -C attach-session -t =w
    }
    cold_socket="/tmp/zzcpa-$$.sock"
    cold_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$cold_socket" "$@"
    }
    cold_control_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$cold_socket" \
            -C attach-session -t =chain-roots
    }
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
    control_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" \
            -C attach-session -t =w
    }
    cold_label="zzcpa-$$"
    cold_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$cold_label" "$@"
    }
    cold_control_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$cold_label" \
            -C attach-session -t =chain-roots
    }
fi

cold_started=0
cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    if [ "$cold_started" -eq 1 ]; then
        cold_client kill-server >/dev/null 2>&1
    fi
    if [ "$side" = zz ]; then
        case "$cold_socket" in
        /tmp/zzcpa-[0-9]*.sock)
            rm -f -- "$cold_socket" "${cold_socket}.identity" "${cold_socket}.lock"
            ;;
        esac
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

work="$HOME/config-chain-parse-abort-$side"
mkdir -p "$work"

main_environment() {
    main_client show-environment -g "$1" 2>/dev/null || :
}

cold_environment() {
    cold_client show-environment -g "$1" 2>/dev/null || :
}

cold_option() {
    cold_client show-options -gqv "$1" 2>/dev/null || :
}

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
    fi
    wait "$wait_pid"
}

wait_for_control_line() {
    wait_file=$1
    wait_line=$2
    wait_pid=$3
    wait_attempt=0
    until grep -Fqx -- "$wait_line" "$wait_file"; do
        wait_attempt=$((wait_attempt + 1))
        if [ "$wait_attempt" -ge 200 ] || ! kill -0 "$wait_pid" 2>/dev/null; then
            return 1
        fi
        sleep 0.05
    done
}

wait_for_control_output() {
    wait_file=$1
    wait_line=$2
    wait_pid=$3
    wait_attempt=0
    until awk -v target="$wait_line" '
        $0 == target { output = 1; next }
        output && /^%end [0-9]+ [0-9]+ 1$/ { complete = 1 }
        END { exit !complete }
    ' "$wait_file"; do
        wait_attempt=$((wait_attempt + 1))
        if [ "$wait_attempt" -ge 200 ] || ! kill -0 "$wait_pid" 2>/dev/null; then
            return 1
        fi
        sleep 0.05
    done
}

direct=broken
direct_config="$work/direct.conf"
direct_out="$work/direct.out"
direct_err="$work/direct.err"
direct_expected="$work/direct.expected"
printf '%s\n' \
    'CHAIN_DIRECT_PARSE=from-failed' \
    'set-environment -g CHAIN_DIRECT_BEFORE wrong' \
    'display-message -p CHAIN_DIRECT_VISIBLE' \
    'set-environment -g' \
    'set-environment -g CHAIN_DIRECT_AFTER wrong' \
    'wibble' \
    >"$direct_config"
printf '%s\n' \
    "$direct_config:2: set-environment -g CHAIN_DIRECT_BEFORE wrong" \
    "$direct_config:3: display-message -p CHAIN_DIRECT_VISIBLE" \
    "$direct_config:4: command set-environment: too few arguments (need at least 1)" \
    >"$direct_expected"
set +e
main_client source-file -v "$direct_config" >"$direct_out" 2>"$direct_err"
direct_status=$?
set -e
if [ "$direct_status" -eq 1 ] && [ ! -s "$direct_err" ] && \
    cmp -s "$direct_expected" "$direct_out" && \
    [ "$(main_environment CHAIN_DIRECT_PARSE)" = \
        'CHAIN_DIRECT_PARSE=from-failed' ] && \
    [ -z "$(main_environment CHAIN_DIRECT_BEFORE)" ] && \
    [ -z "$(main_environment CHAIN_DIRECT_AFTER)" ]; then
    direct=clean
fi

nv=broken
nv_config="$work/parse-only.conf"
nv_out="$work/parse-only.out"
nv_err="$work/parse-only.err"
nv_expected="$work/parse-only.expected"
main_client set-environment -g CHAIN_NV_VALUE old
main_client set-option -s 'command-alias[99]' \
    'chain_nv_alias=set-environment -g CHAIN_NV_ALIAS $CHAIN_NV_VALUE'
printf '%s\n' \
    'CHAIN_NV_PARSE=wrong' \
    'CHAIN_NV_VALUE=new' \
    'chain_nv_alias' \
    'set-environment -g CHAIN_NV_BEFORE wrong' \
    'set-environment -g' \
    'set-environment -g CHAIN_NV_AFTER wrong' \
    >"$nv_config"
printf '%s\n' \
    "$nv_config:3: set-environment -g CHAIN_NV_ALIAS old" \
    "$nv_config:3: set-environment -g CHAIN_NV_ALIAS old" \
    "$nv_config:4: set-environment -g CHAIN_NV_BEFORE wrong" \
    "$nv_config:5: command set-environment: too few arguments (need at least 1)" \
    >"$nv_expected"
set +e
main_client source-file -nv "$nv_config" >"$nv_out" 2>"$nv_err"
nv_status=$?
set -e
if [ "$nv_status" -eq 1 ] && [ ! -s "$nv_err" ] && \
    cmp -s "$nv_expected" "$nv_out" && \
    [ -z "$(main_environment CHAIN_NV_PARSE)" ] && \
    [ "$(main_environment CHAIN_NV_VALUE)" = 'CHAIN_NV_VALUE=old' ] && \
    [ -z "$(main_environment CHAIN_NV_ALIAS)" ] && \
    [ -z "$(main_environment CHAIN_NV_BEFORE)" ] && \
    [ -z "$(main_environment CHAIN_NV_AFTER)" ]; then
    nv=clean
fi

batch=broken
batch_first="$work/batch-first.conf"
batch_invalid="$work/batch-invalid.conf"
batch_later="$work/batch-later.conf"
batch_out="$work/batch.out"
batch_err="$work/batch.err"
batch_expected="$work/batch.expected"
printf '%s\n' \
    'display-message -p BATCH_FIRST' \
    'set-environment -g CHAIN_BATCH_FIRST yes' \
    >"$batch_first"
printf '%s\n' \
    'CHAIN_BATCH_PARSE=from-invalid' \
    'set-environment -g CHAIN_BATCH_BAD_BEFORE wrong' \
    'set-environment -g' \
    'set-environment -g CHAIN_BATCH_BAD_AFTER wrong' \
    >"$batch_invalid"
printf '%s\n' \
    'display-message -p "BATCH_$CHAIN_BATCH_PARSE"' \
    'set-environment -g CHAIN_BATCH_LATER yes' \
    >"$batch_later"
printf '%s\n' \
    'BATCH_FIRST' \
    'BATCH_from-invalid' \
    "$batch_invalid:3: command set-environment: too few arguments (need at least 1)" \
    >"$batch_expected"
set +e
main_client source-file "$batch_first" "$batch_invalid" "$batch_later" \
    >"$batch_out" 2>"$batch_err"
batch_status=$?
set -e
if [ "$batch_status" -eq 1 ] && [ ! -s "$batch_err" ] && \
    cmp -s "$batch_expected" "$batch_out" && \
    [ "$(main_environment CHAIN_BATCH_FIRST)" = \
        'CHAIN_BATCH_FIRST=yes' ] && \
    [ "$(main_environment CHAIN_BATCH_PARSE)" = \
        'CHAIN_BATCH_PARSE=from-invalid' ] && \
    [ "$(main_environment CHAIN_BATCH_LATER)" = \
        'CHAIN_BATCH_LATER=yes' ] && \
    [ -z "$(main_environment CHAIN_BATCH_BAD_BEFORE)" ] && \
    [ -z "$(main_environment CHAIN_BATCH_BAD_AFTER)" ]; then
    batch=clean
fi

nested=broken
nested_child="$work/nested-child.conf"
nested_parent="$work/nested-parent.conf"
nested_out="$work/nested.out"
nested_err="$work/nested.err"
nested_expected="$work/nested.expected"
printf '%s\n' \
    'CHAIN_CHILD_PARSE=from-child' \
    'set-environment -g CHAIN_CHILD_EFFECT wrong' \
    'set-environment -g' \
    'set-environment -g CHAIN_CHILD_LATE wrong' \
    >"$nested_child"
printf '%s\n' \
    'set-environment -g CHAIN_PARENT_BEFORE yes' \
    "source-file '$nested_child' ; set-environment -g CHAIN_PARENT_SAME yes" \
    'set-environment -g CHAIN_PARENT_AFTER yes' \
    >"$nested_parent"
printf '%s\n' \
    "$nested_child:3: command set-environment: too few arguments (need at least 1)" \
    >"$nested_expected"
set +e
main_client source-file "$nested_parent" >"$nested_out" 2>"$nested_err"
nested_status=$?
set -e
if [ "$nested_status" -eq 1 ] && [ ! -s "$nested_err" ] && \
    cmp -s "$nested_expected" "$nested_out" && \
    [ "$(main_environment CHAIN_PARENT_BEFORE)" = \
        'CHAIN_PARENT_BEFORE=yes' ] && \
    [ "$(main_environment CHAIN_PARENT_SAME)" = \
        'CHAIN_PARENT_SAME=yes' ] && \
    [ "$(main_environment CHAIN_PARENT_AFTER)" = \
        'CHAIN_PARENT_AFTER=yes' ] && \
    [ "$(main_environment CHAIN_CHILD_PARSE)" = \
        'CHAIN_CHILD_PARSE=from-child' ] && \
    [ -z "$(main_environment CHAIN_CHILD_EFFECT)" ] && \
    [ -z "$(main_environment CHAIN_CHILD_LATE)" ]; then
    nested=clean
fi

startup=broken
root_first="$work/root-first.conf"
root_invalid="$work/root-invalid.conf"
root_later="$work/root-later.conf"
cold_out="$work/cold.out"
cold_err="$work/cold.err"
cold_control_raw="$work/cold-control.raw"
cold_control_err="$work/cold-control.err"
cold_control_normalized="$work/cold-control.normalized"
cold_control_expected="$work/cold-control.expected"
printf '%s\n' \
    'set-option -g @chain-root-first yes' \
    >"$root_first"
printf '%s\n' \
    'CHAIN_ROOT_PARSE=from-invalid-root' \
    'set-option -g @chain-root-bad-before wrong' \
    'set-environment -g' \
    'set-option -g @chain-root-bad-after wrong' \
    >"$root_invalid"
printf '%s\n' \
    'set-option -g @chain-root-later "$CHAIN_ROOT_PARSE"' \
    >"$root_later"
cold_started=1
set +e
cold_client -f "$root_first" -f "$root_invalid" -f "$root_later" \
    new-session -d -s chain-roots >"$cold_out" 2>"$cold_err"
cold_status=$?
set -e
if [ "$cold_status" -eq 0 ] && [ ! -s "$cold_out" ] && [ ! -s "$cold_err" ]; then
    set +e
    printf '%s\n' 'detach-client' | cold_control_client \
        >"$cold_control_raw" 2>"$cold_control_err"
    cold_control_status=$?
    set -e
    startup_diagnostic="%config-error $root_invalid:3: command set-environment: too few arguments (need at least 1)"
    awk -v expected="$startup_diagnostic" '
        !started && /^%begin [0-9]+ [0-9]+ 0$/ {
            started = 1
            print "begin-0"
            next
        }
        started && !cause && $0 == expected {
            cause = 1
            print "config-error"
            next
        }
        cause && !ended && /^%end [0-9]+ [0-9]+ 0$/ {
            ended = 1
            print "end-0"
            next
        }
        ended && /^%session-changed / {
            print "session-changed"
            exit
        }
    ' "$cold_control_raw" >"$cold_control_normalized"
    printf '%s\n' \
        'begin-0' \
        'config-error' \
        'end-0' \
        'session-changed' \
        >"$cold_control_expected"
    startup_config_errors="$(awk '
        /^%config-error / { count++ }
        END { print count + 0 }
    ' "$cold_control_raw")"
    if [ "$cold_control_status" -eq 0 ] && \
        [ ! -s "$cold_control_err" ] && \
        [ "$startup_config_errors" -eq 1 ] && \
        cmp -s "$cold_control_expected" "$cold_control_normalized" && \
        [ "$(cold_option @chain-root-first)" = yes ] && \
        [ "$(cold_option @chain-root-later)" = from-invalid-root ] && \
        [ -z "$(cold_option @chain-root-bad-before)" ] && \
        [ -z "$(cold_option @chain-root-bad-after)" ] && \
        [ "$(cold_environment CHAIN_ROOT_PARSE)" = \
            'CHAIN_ROOT_PARSE=from-invalid-root' ]; then
        startup=clean
    fi
fi

control=broken
control_top_good="$work/control-top-good.conf"
control_top_bad="$work/control-top-bad.conf"
control_top_later="$work/control-top-later.conf"
control_nested_good="$work/control-nested-good.conf"
control_nested_bad="$work/control-nested-bad.conf"
control_nested_later="$work/control-nested-later.conf"
control_nested_root="$work/control-nested-root.conf"
control_raw="$work/control.raw"
control_err="$work/control.err"
control_input="$work/control.in"
control_normalized="$work/control.normalized"
control_expected="$work/control.expected"
printf '%s\n' \
    'display-message -p CHAIN_CONTROL_TOP_GOOD' \
    >"$control_top_good"
printf '%s\n' \
    'set-environment -g CHAIN_CONTROL_TOP_BAD_BEFORE wrong' \
    'wibble' \
    'set-environment -g CHAIN_CONTROL_TOP_BAD_AFTER wrong' \
    >"$control_top_bad"
printf '%s\n' \
    'display-message -p CHAIN_CONTROL_TOP_LATER' \
    >"$control_top_later"
printf '%s\n' \
    'display-message -p CHAIN_CONTROL_NESTED_GOOD' \
    >"$control_nested_good"
printf '%s\n' \
    'set-environment -g CHAIN_CONTROL_NESTED_BAD_BEFORE wrong' \
    'wibble' \
    'set-environment -g CHAIN_CONTROL_NESTED_BAD_AFTER wrong' \
    >"$control_nested_bad"
printf '%s\n' \
    'display-message -p CHAIN_CONTROL_NESTED_LATER' \
    >"$control_nested_later"
printf '%s\n' \
    "source-file '$control_nested_good' '$control_nested_bad' '$control_nested_later'" \
    >"$control_nested_root"
control_top_diagnostic="%config-error $control_top_bad:2: unknown command: wibble"
control_nested_diagnostic="%config-error $control_nested_bad:2: unknown command: wibble"
rm -f -- "$control_input"
mkfifo "$control_input"
control_client <"$control_input" >"$control_raw" 2>"$control_err" &
control_pid=$!
exec 3>"$control_input"
control_progress=clean
printf "source-file '%s' '%s' '%s'\n" \
    "$control_top_good" "$control_top_bad" "$control_top_later" >&3
if ! wait_for_control_line "$control_raw" "$control_top_diagnostic" "$control_pid"; then
    control_progress=broken
fi
if [ "$control_progress" = clean ]; then
    printf "source-file '%s'\n" "$control_nested_root" >&3
    if ! wait_for_control_line \
        "$control_raw" "$control_nested_diagnostic" "$control_pid"; then
        control_progress=broken
    fi
fi
if [ "$control_progress" = clean ]; then
    printf '%s\n' 'display-message -p CHAIN_CONTROL_DONE' >&3
    if ! wait_for_control_output "$control_raw" CHAIN_CONTROL_DONE "$control_pid"; then
        control_progress=broken
    fi
fi
if [ "$control_progress" = clean ]; then
    printf '%s\n' 'detach-client' >&3
fi
exec 3>&-
set +e
wait_for_process "$control_pid" 200
control_status=$?
set -e
awk -v top_error="$control_top_diagnostic" \
    -v nested_error="$control_nested_diagnostic" '
    !finished && /^%begin [0-9]+ [0-9]+ 1$/ {
        started = 1
        print "begin-1"
        next
    }
    started && !finished && /^%end [0-9]+ [0-9]+ 1$/ {
        print "end-1"
        if (done)
            finished = 1
        next
    }
    started && !finished && /^%error [0-9]+ [0-9]+ 1$/ {
        print "error-1"
        if (done)
            finished = 1
        next
    }
    started && !finished && $0 == top_error {
        print "config-error-top"
        next
    }
    started && !finished && $0 == nested_error {
        print "config-error-nested"
        next
    }
    started && !finished && /^CHAIN_CONTROL_(TOP|NESTED)_(GOOD|LATER)$/ {
        print
        next
    }
    started && !finished && $0 == "CHAIN_CONTROL_DONE" {
        done = 1
        print
        next
    }
' "$control_raw" >"$control_normalized"
printf '%s\n' \
    'begin-1' \
    'end-1' \
    'begin-1' \
    'CHAIN_CONTROL_TOP_GOOD' \
    'end-1' \
    'begin-1' \
    'CHAIN_CONTROL_TOP_LATER' \
    'end-1' \
    'config-error-top' \
    'begin-1' \
    'end-1' \
    'begin-1' \
    'end-1' \
    'begin-1' \
    'CHAIN_CONTROL_NESTED_GOOD' \
    'end-1' \
    'begin-1' \
    'CHAIN_CONTROL_NESTED_LATER' \
    'end-1' \
    'config-error-nested' \
    'begin-1' \
    'CHAIN_CONTROL_DONE' \
    'end-1' \
    >"$control_expected"
control_config_errors="$(awk '
    /^%config-error / { count++ }
    END { print count + 0 }
' "$control_raw")"
if [ "$control_progress" = clean ] && [ "$control_status" -eq 0 ] && \
    [ ! -s "$control_err" ] && \
    [ "$control_config_errors" -eq 2 ] && \
    cmp -s "$control_expected" "$control_normalized" && \
    [ -z "$(main_environment CHAIN_CONTROL_TOP_BAD_BEFORE)" ] && \
    [ -z "$(main_environment CHAIN_CONTROL_TOP_BAD_AFTER)" ] && \
    [ -z "$(main_environment CHAIN_CONTROL_NESTED_BAD_BEFORE)" ] && \
    [ -z "$(main_environment CHAIN_CONTROL_NESTED_BAD_AFTER)" ]; then
    control=clean
fi

runtime=broken
runtime_config="$work/runtime.conf"
runtime_out="$work/runtime.out"
runtime_err="$work/runtime.err"
runtime_expected="$work/runtime.expected"
printf '%s\n' \
    'set-environment -g CHAIN_RUNTIME_BEFORE yes' \
    'kill-session -t missing-chain ; set-environment -g CHAIN_RUNTIME_SAME wrong' \
    'set-environment -g CHAIN_RUNTIME_AFTER yes' \
    >"$runtime_config"
printf '%s\n' "can't find session: missing-chain" >"$runtime_expected"
set +e
main_client source-file "$runtime_config" >"$runtime_out" 2>"$runtime_err"
runtime_status=$?
set -e
if [ "$runtime_status" -eq 1 ] && [ ! -s "$runtime_out" ] && \
    cmp -s "$runtime_expected" "$runtime_err" && \
    [ "$(main_environment CHAIN_RUNTIME_BEFORE)" = \
        'CHAIN_RUNTIME_BEFORE=yes' ] && \
    [ -z "$(main_environment CHAIN_RUNTIME_SAME)" ] && \
    [ "$(main_environment CHAIN_RUNTIME_AFTER)" = \
        'CHAIN_RUNTIME_AFTER=yes' ]; then
    runtime=clean
fi

main_client set-environment -g CONFIG_CHAIN_PARSE_ABORT \
    "direct:$direct,nv:$nv,batch:$batch,nested:$nested,startup:$startup,control:$control,runtime:$runtime"
