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
input="$HOME/control-alias-prepare.in"

wait_for() {
    tries=0
    until "$@"; do
        tries=$((tries + 1))
        [ "$tries" -lt 200 ] || return 0
        sleep 0.05
    done
}

alias_outputs() {
    [ "$(grep -cxE 'old|new' "$raw")" -ge "$1" ]
}

next_is_set() {
    main_client show-environment -g CONTROL_NEXT >/dev/null 2>&1
}

rm -f "$input"
mkfifo "$input"
control_client <"$input" >"$raw" 2>"$errors" &
control=$!
exec 3>"$input"
printf '%s\n' "set-option -s command-alias[40] 'live=display-message -p new' ; live" >&3
wait_for alias_outputs 1
printf '%s\n' 'live' >&3
wait_for alias_outputs 2
printf '%s\n' 'set-environment -g CONTROL_NEXT visible' >&3
wait_for next_is_set
printf '%s\n' 'display-message -p $CONTROL_NEXT' >&3
printf '%s\n' 'set-environment -g CONTROL_BEFORE bad ; frobnicate ; set-environment -g CONTROL_AFTER bad' >&3
printf '%s\n' 'detach-client' >&3
exec 3>&-
wait "$control"

if grep -qx old "$raw" && grep -qx new "$raw" && [ "$(main_client live)" = new ]; then
    main_client set-environment -g CONTROL_ALIAS_SNAPSHOT frozen
else
    main_client set-environment -g CONTROL_ALIAS_SNAPSHOT broken
fi

if grep -qx visible "$raw"; then
    main_client set-environment -g CONTROL_EXPANSION_ORDER sequential
else
    main_client set-environment -g CONTROL_EXPANSION_ORDER broken
fi

if ! main_client show-environment -g CONTROL_BEFORE >/dev/null 2>&1 && \
   ! main_client show-environment -g CONTROL_AFTER >/dev/null 2>&1; then
    main_client set-environment -g CONTROL_PREPARE_ABORT clean
else
    main_client set-environment -g CONTROL_PREPARE_ABORT mutated
fi
