#!/usr/bin/env bash
set -e
W=/tmp/zzprobe-sa-home; rm -rf "$W"; mkdir -p "$W/config"
export HOME="$W" XDG_CONFIG_HOME="$W/config" TMUX_TMPDIR=/tmp
T=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux
S=zzprobe-sa-$$
$T -L "$S" -f /dev/null new-session -d -s p -x 80 -y 24
run() { printf '### %s\n' "$*"; set +e; out=$($T -L "$S" -f /dev/null "$@" 2>/tmp/e); rc=$?; set -e; printf 'rc=%s out=[%s] err=[%s]\n' "$rc" "$out" "$(cat /tmp/e)"; }
run server-access
run server-access -t %0
run server-access -l
run server-access -l extra
run server-access "$(id -un)"
run server-access -a "$(id -un)"
run server-access root
run server-access -g "$(id -gn)"
run server-access -a -g "$(id -gn)"
run server-access -l
run server-access -a -d "$(id -gn)"
run server-access -w -r nobody
run server-access -d nobody
run server-access -g -a -d "$(id -gn)"
run server-access nosuchuser
run server-access -g nosuchgroup
run server-access -a -g nosuchgroup
run server-access one two
$T -L "$S" -f /dev/null kill-server 2>/dev/null || true
rm -rf "$W" /tmp/e
