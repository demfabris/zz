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

main_client set-option -s command-alias[40] 'live=display-message -p old'
raw="$HOME/control-alias-prepare.raw"
errors="$HOME/control-alias-prepare.err"
{
    printf '%s\n' "set-option -s command-alias[40] 'live=display-message -p new' ; live"
    printf '%s\n' 'set-environment -g CONTROL_BEFORE bad ; frobnicate ; set-environment -g CONTROL_AFTER bad'
    printf '%s\n' 'detach-client'
} | control_client >"$raw" 2>"$errors"

if grep -qx old "$raw" && [ "$(main_client live)" = new ]; then
    main_client set-environment -g CONTROL_ALIAS_SNAPSHOT frozen
else
    main_client set-environment -g CONTROL_ALIAS_SNAPSHOT broken
fi

if ! main_client show-environment -g CONTROL_BEFORE >/dev/null 2>&1 && \
   ! main_client show-environment -g CONTROL_AFTER >/dev/null 2>&1; then
    main_client set-environment -g CONTROL_PREPARE_ABORT clean
else
    main_client set-environment -g CONTROL_PREPARE_ABORT mutated
fi
