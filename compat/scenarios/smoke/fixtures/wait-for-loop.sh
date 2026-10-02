#!/bin/sh
set -eu
export LC_ALL=C
if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    binary="$ZZ_SMOKE_ZZ_BIN"
    set -- --socket "$ZZ_SMOKE_ZZ_SOCKET"
else
    binary="$ZZ_SMOKE_TMUX_BIN"
    set -- -L "$ZZ_SMOKE_TMUX_LABEL"
fi
python3 "$HOME/wait-for-loop.py" "$binary" "$@"
"$binary" "$@" set-environment -g WAIT_FOR_LOOP blocking-signal-lock-fifo-unlock-order
