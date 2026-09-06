#!/bin/sh
set -eu
if [ -n "${ZZ_SMOKE_TMUX_LABEL:-}" ]; then
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    set -- -L "$ZZ_SMOKE_TMUX_LABEL"
else
    side=zz
    binary=zz
    set --
fi
work="$HOME/pane-tmux-path-$side"
mkdir -p "$work"
if python3 "$HOME/pane-tmux-path.py" "$work" "$binary" "$@" >"$work/observed" 2>"$work/errors"; then
    cat "$work/observed"
else
    echo "pane-tmux-path-$side failed"
    cat "$work/observed" "$work/errors"
    exit 1
fi
