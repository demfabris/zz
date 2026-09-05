#!/bin/sh
set -eu
if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    set -- --socket "$ZZ_SMOKE_ZZ_SOCKET"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    set -- -L "$ZZ_SMOKE_TMUX_LABEL"
fi
work="$HOME/keys-contract-$side"
mkdir -p "$work"
if python3 "$HOME/keys-contract.py" "$work" "$binary" "$@" >"$work/observed" 2>"$work/errors"; then
    cat "$work/observed"
    "$binary" "$@" set-environment -g KEYS_CONTRACT clean:5
else
    echo "keys-contract-$side failed"
    cat "$work/observed" "$work/errors"
fi
