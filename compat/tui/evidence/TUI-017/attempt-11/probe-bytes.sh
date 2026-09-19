#!/usr/bin/env bash
# One-shot byte probes for the unpatch lane: exact capture-pane bytes on both
# sides for the two cases losing their asserted basis. No loops over commands,
# no subprocess wrapper: fixed commands, fixed payloads.
set -u
REPO=/home/demfabris/dev/zz-c11-capture-residuals
cd "$REPO" || exit 2
scrub() {
  env -u GITHUB_PERSONAL_ACCESS_TOKEN -u GH_TOKEN -u GITHUB_TOKEN \
    -u CLAUDE_CODE_MESSAGING_TOKEN -u CLAUDE_CODE_SESSION_ID \
    -u CLAUDE_CODE_CHILD_SESSION -u ANTHROPIC_API_KEY -u OPENAI_API_KEY \
    -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE "$@"
}
ZZ=$REPO/target/debug/zz_cli
TMUX=$REPO/compat/.cache/tmux-src/tmux
SOCK=/tmp/unpatch-probe.sock
OUT=$REPO/target/scratch-unpatch
mkdir -p "$OUT" /tmp/unpatch-home/zz /tmp/unpatch-home/tmux
rm -f "$SOCK"
setsid -f env HOME=/tmp/unpatch-home/zz XDG_CONFIG_HOME=/tmp/unpatch-home/zz/config \
  ZZ_LOG_DIR=$OUT/logs "$ZZ" --socket "$SOCK" -f /dev/null daemon </dev/null >>"$OUT/daemon.out" 2>&1
for _ in $(seq 1 200); do [ -S "$SOCK" ] && break; sleep 0.05; done
[ -S "$SOCK" ] || { echo "no zz socket"; exit 2; }
zz() { scrub env HOME=/tmp/unpatch-home/zz XDG_CONFIG_HOME=/tmp/unpatch-home/zz/config \
  ZZ_LOG_DIR=$OUT/logs "$ZZ" --socket "$SOCK" "$@"; }
tm() { scrub env HOME=/tmp/unpatch-home/tmux XDG_CONFIG_HOME=/tmp/unpatch-home/tmux/config \
  TMUX_TMPDIR=/tmp "$TMUX" -L unpatch-probe "$@"; }
tm kill-server >/dev/null 2>&1 || true

probe() {
  local name="$1" payload="$2"
  shift 2
  local cmd
  printf -v cmd 'printf %%b %q; exec sleep 600' "$payload"
  tm -f /dev/null new-session -d -s probe -x 80 -y 24 "$cmd" >/dev/null
  zz new-session -d -s probe -x 80 -y 24 "$cmd" >/dev/null
  for _ in $(seq 1 200); do
    tm capture-pane -p -t '=probe:0' 2>/dev/null | grep -Fq NEXT && break
    sleep 0.05
  done
  for _ in $(seq 1 200); do
    zz capture-pane -p -t '=probe:0' 2>/dev/null | grep -Fq NEXT && break
    sleep 0.05
  done
  tm capture-pane -p -t '=probe:0' "$@" >"$OUT/$name.tmux.raw" 2>"$OUT/$name.tmux.err"
  echo "tmux exit $?" >"$OUT/$name.tmux.meta"
  zz capture-pane -p -t '=probe:0' "$@" >"$OUT/$name.zz.raw" 2>"$OUT/$name.zz.err"
  echo "zz exit $?" >>"$OUT/$name.zz.meta"
  tm kill-session -t '=probe:0' >/dev/null 2>&1 || true
  zz kill-session -t '=probe:0' >/dev/null 2>&1 || true
  printf '%s\n' "=== $name tmux ($(wc -c <"$OUT/$name.tmux.raw") bytes) ==="
  xxd "$OUT/$name.tmux.raw" | head -20
  printf '%s\n' "=== $name zz ($(wc -c <"$OUT/$name.zz.raw") bytes) ==="
  xxd "$OUT/$name.zz.raw" | head -20
  cmp "$OUT/$name.tmux.raw" "$OUT/$name.zz.raw" >/dev/null 2>&1 && echo SAME || echo DIFFER
}

probe low-indexed '\033[38;5;1mRED\033[0m\r\nNEXT' -C -e -S 0 -E 0
probe ich-off-line '\033[73GABC\tZ\r\033[70G\033[6@\033[5;1HNEXT' -C -S 0 -E 4

zz kill-server >/dev/null 2>&1 || true
tm kill-server >/dev/null 2>&1 || true
rm -f "$SOCK"
echo PROBE-DONE
