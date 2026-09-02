#!/bin/sh
# What a Control client's command queue does when its input reaches end of
# file: tmux runs every queued command up to and including the first one that
# yields the queue (CMD_RETURN_WAIT), then the client exits and the queue it
# owned is freed with everything later unrun.
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" \
            -C attach-session -t '=w'
    }
else
    side=tmux
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" \
            -C attach-session -t '=w'
    }
fi

work="$HOME/control-eof-drain-$side"
rm -rf "$work"
mkdir -p "$work"

markers() {
    ls "$work" 2>/dev/null | grep '^m' | sort | tr '\n' ' ' | sed 's/ $//'
}

clear_markers() {
    rm -f "$work"/m*
}

# The guard numbers are a server-wide counter on the pin and a per-client one in
# zz, and the times are wall clock, so the comparable fact is the guard shape.
normalize() {
    sed -E \
        -e 's/^%(begin|end|error) [0-9]+ [0-9]+ ([0-9]+)$/%\1 \2/' \
        -e 's/^%session-changed \$[0-9]+ /%session-changed /'
}

# `at-exit` is only reported for the cases whose job sleeps: whether an instant
# job has already touched its marker when the client exits is a race on both
# binaries, and what the queue ran is the fact under test.
drive() {
    label="$1"
    late="$2"
    at_exit_reported="$3"
    shift 3
    clear_markers
    raw="$work/$label.raw"
    err="$work/$label.err"
    set +e
    printf '%s\n' "$@" | control_client >"$raw" 2>"$err"
    rc=$?
    set -e
    at_exit="$(markers)"
    if [ "$late" -eq 1 ]; then
        sleep 3
    fi
    if [ "$at_exit_reported" -eq 1 ]; then
        printf '%s rc=%s at-exit=[%s] late=[%s]\n' "$label" "$rc" "$at_exit" "$(markers)"
    else
        printf '%s rc=%s late=[%s]\n' "$label" "$rc" "$(markers)"
    fi
    normalize <"$raw" | sed "s/^/$label | /"
    if [ -s "$err" ]; then
        sed "s/^/$label ! /" "$err"
    fi
}

drive three-messages 0 0 \
    'display-message -p ONE' \
    'display-message -p TWO' \
    'display-message -p THREE'

drive block-first 1 1 \
    "run-shell 'sleep 2; touch $work/m1'" \
    'display-message -p SECOND' \
    'display-message -p THIRD'

drive block-last 1 1 \
    'display-message -p ONE' \
    'display-message -p TWO' \
    "run-shell 'sleep 2; touch $work/m3'"

# A blank Return ends the client the same way end of file does.
drive blank-return 1 1 \
    "run-shell 'sleep 2; touch $work/m1'" \
    '' \
    'display-message -p AFTER'

drive fast-run-first 1 0 \
    "run-shell 'touch $work/m1'" \
    'display-message -p AFTER'

# `run-shell 'echo hi'` belongs in this list and is left out on purpose: at
# d77c9dc6 the pinned server dies when the next client connects after a Control
# client exits at end of file with a job whose output has nowhere to go.

drive size-reports 0 0 \
    'refresh-client -C 120,40' \
    'refresh-client -C 120,40' \
    'display-message -p SIZE' \
    'detach-client'

# A sourced queue is the same queue: the pin discards what follows the sourced
# command that parked. Only the payload lines and the side effects are compared
# here, because zz closes a sourced command's control guard when the command
# finishes and the pin closes it when the command fires.
conf="$work/queue.conf"
{
    printf '%s\n' 'display-message -p SRC-A'
    printf '%s\n' 'display-message -p SRC-B'
    printf "run-shell 'sleep 2; touch %s'\n" "$work/m3"
    printf '%s\n' 'display-message -p SRC-C'
} >"$conf"
clear_markers
set +e
printf 'source-file %s\n' "$conf" | control_client >"$work/source.raw" 2>"$work/source.err"
rc=$?
set -e
at_exit="$(markers)"
sleep 3
printf 'source-park rc=%s at-exit=[%s] late=[%s]\n' "$rc" "$at_exit" "$(markers)"
grep -E '^(SRC-|parse error)' "$work/source.raw" | sed 's/^/source-park | /' || true
if [ -s "$work/source.err" ]; then
    sed 's/^/source-park ! /' "$work/source.err"
fi
