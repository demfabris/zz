#!/usr/bin/env bash
set -u
export ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux
export ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins
export ZZ_COMPAT_ZZ=/home/demfabris/dev/zz-box-reds/target/debug/zz
export TMUX_BIN="$ZZ_COMPAT_TMUX"
export ZZ_TRAY=0
export CARGO_HOME=/home/demfabris/.cargo RUSTUP_HOME=/home/demfabris/.rustup
export HOME=/tmp/zz-emptyhome XDG_CONFIG_HOME=/tmp/zz-emptyhome/config
label=$1
shift
E=compat/tui/evidence/TUI-018/attempt-02
printf -v command '%q ' "$@"
printf '%s START tip=%s %s\n' "$(date -u +%FT%TZ)" "$(git rev-parse HEAD)" "$command" >> "$E/final-commands.txt"
"$@" > "$E/final-$label.txt" 2>&1
rc=$?
printf '%s END exit=%s %s\n' "$(date -u +%FT%TZ)" "$rc" "$command" >> "$E/final-commands.txt"
printf '%s exit=%s\n' "$label" "$rc"
tail -8 "$E/final-$label.txt"
exit "$rc"
