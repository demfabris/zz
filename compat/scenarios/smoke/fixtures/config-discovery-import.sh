#!/bin/sh
set -eu
cp "$XDG_CONFIG_HOME/zz/mux.conf" "$HOME/mux-before-import"
tmux import-tmux-config > "$HOME/import-message"
if ! cmp "$HOME/mux-before-import" "$XDG_CONFIG_HOME/zz/mux.conf" ||
   ! grep -q 'reads tmux configuration files in place' "$HOME/import-message"; then
    echo 'import-tmux-config changed mux.conf or omitted the in-place explanation' >&2
    exit 1
fi
