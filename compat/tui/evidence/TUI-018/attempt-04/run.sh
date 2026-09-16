#!/usr/bin/env bash
export ZZ_COMPAT_TMUX="$PWD/compat/.cache/tmux-src/tmux"
export ZZ_COMPAT_CORPUS="$PWD/compat/.cache/plugins"
export ZZ_COMPAT_ZZ="$PWD/target/debug/zz"
export TMUX_BIN="$ZZ_COMPAT_TMUX" ZZ_BIN="$ZZ_COMPAT_ZZ" ZZ_TRAY=0
export CARGO_HOME=/home/demfabris/.cargo RUSTUP_HOME=/home/demfabris/.rustup
export HOME=/tmp/zz-emptyhome XDG_CONFIG_HOME=/tmp/zz-emptyhome/config
unset ZZ_DEV_BUILD ZZ_SOCKET ZZ_PANE TMUX
export PATH=/tmp/zz-c11-alias-tools:$PATH
label=$1
shift
E=compat/tui/evidence/TUI-018/attempt-04
printf -v command '%q ' "$@"
printf '%s START %s tip=%s %s\n' "$(date -u +%FT%TZ)" "$label" "$(git rev-parse HEAD)" "$command" >> "$E/commands.txt"
if [ "$1" = /tmp/zz-cargo.sh ]; then export ZZ_ALIAS_CARGO_SCOPED=1; fi
"$@" > "$E/$label.txt" 2>&1
rc=$?
printf '%s END %s exit=%s\n' "$(date -u +%FT%TZ)" "$label" "$rc" >> "$E/commands.txt"
printf '%s exit=%s\n' "$label" "$rc"
exit "$rc"
