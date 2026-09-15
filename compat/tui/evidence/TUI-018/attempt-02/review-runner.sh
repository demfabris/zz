#!/bin/bash
export ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux
export ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins
export ZZ_COMPAT_ZZ=/home/demfabris/dev/zz-box-reds/target/debug/zz
export TMUX_BIN="$ZZ_COMPAT_TMUX" ZZ_BIN="$ZZ_COMPAT_ZZ" ZZ_TRAY=0
export CARGO_HOME=/home/demfabris/.cargo RUSTUP_HOME=/home/demfabris/.rustup
export HOME=/tmp/zz-emptyhome XDG_CONFIG_HOME=/tmp/zz-emptyhome/config
export PATH=/tmp/zz018-review-tools:$PATH
unset ZZ_DEV_BUILD ZZ_SOCKET ZZ_PANE TMUX
label=$1
shift
E=compat/tui/evidence/TUI-018/attempt-02
printf -v command '%q ' "$@"
printf '%s START label=%s tip=%s %s\n' "$(date -u +%FT%TZ)" "$label" "$(git rev-parse HEAD)" "$command" >> "$E/review-commands.txt"
"$@" > "$E/review-$label.txt" 2>&1
rc=$?
printf '%s END label=%s exit=%s\n' "$(date -u +%FT%TZ)" "$label" "$rc" >> "$E/review-commands.txt"
printf '%s exit=%s\n' "$label" "$rc"
exit "$rc"
