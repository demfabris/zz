#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    set -- --socket "$ZZ_SMOKE_ZZ_SOCKET"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    set -- -L "$ZZ_SMOKE_TMUX_LABEL"
fi

work="$HOME/control-hard-loss-$side"
rm -rf "$work"
mkdir -p "$work"

# shellcheck disable=SC2086
python3 "$HOME/control-hard-loss.py" "$work" "$binary" "$@" >"$work/observed" 2>"$work/errors"

if [ -s "$work/errors" ]; then
    sed "s/^/control-hard-loss-$side: /" "$work/errors"
    exit 0
fi

sed 's/^/observed /' "$work/observed"
