#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" \
            -C attach-session -t w
    }
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" \
            -C attach-session -t w
    }
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

work="$HOME/control-diagnostics-work"
mkdir -p "$work"
config="$work/diagnostic.conf"
output="$work/control.out"
errors="$work/control.err"
printf '%s\n' wibble >"$config"

set +e
{
    printf "source-file '%s'\n" "$config"
    printf '%s\n' detach-client
} | control_client >"$output" 2>"$errors"
status=$?
set -e

if [ "$status" -eq 0 ] && [ ! -s "$errors" ] && awk -v expected="$config:1: unknown command: wibble" '
    /^%begin [0-9]+ [0-9]+ 1$/ {
        in_frame = 1
        source_begin = NR
        source_payload = 0
        next
    }
    /^%end [0-9]+ [0-9]+ 1$/ && in_frame {
        in_frame = 0
        if (!source_payload && !source_end && !diagnostics) source_end = NR
        next
    }
    in_frame { source_payload = 1 }
    /^%config-error / {
        diagnostics++
        diagnostic_line = NR
        diagnostic_text = substr($0, 15)
        diagnostic_in_frame = in_frame
    }
    /^%exit$/ { exit_line = NR }
    END {
        exit !(source_end && diagnostics == 1 && diagnostic_text == expected &&
               !diagnostic_in_frame && source_end < diagnostic_line &&
               diagnostic_line < exit_line)
    }
' "$output"; then
    main_client set-environment -g CONTROL_DIAGNOSTICS clean
else
    failure_side="${ZZ_SMOKE_CANARY:-missing-canary}"
    main_client set-environment -g CONTROL_DIAGNOSTICS "failed:$failure_side"
    printf '%s\n' "failed:$failure_side" >&2
fi
