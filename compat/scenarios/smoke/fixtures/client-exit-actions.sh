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

work="$HOME/client-exit-actions-$side"
rm -rf "$work"
mkdir -p "$work"

# shellcheck disable=SC2086
python3 "$HOME/client-exit-actions.py" "$work" "$binary" "$@" >"$work/observed" 2>"$work/errors"

if [ -s "$work/errors" ]; then
    sed "s/^/client-exit-actions-$side: /" "$work/errors"
    exit 0
fi

lines="$(wc -l <"$work/observed" | tr -d ' ')"
if [ "$lines" != 6 ]; then
    echo "client-exit-actions-$side: expected 6 observations, got $lines"
    cat "$work/observed"
    exit 0
fi

# The observations themselves are the differential; the environment value only
# reports that the run produced a full set.
sed 's/^/observed /' "$work/observed"
"$binary" "$@" set-environment -g CLIENT_EXIT_ACTIONS "observed:$lines"
