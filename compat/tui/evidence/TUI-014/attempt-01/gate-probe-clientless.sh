#!/usr/bin/env bash
set -u
TMUX_BIN=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux
ZZ_BIN=/home/demfabris/dev/zz-gate-target/debug/zz
W=$(mktemp -d /tmp/zzprobe.XXXXXX); TOK=${W##*.}; SOCK=/tmp/zzprobe-$TOK.sock
ZH=$W/zzhome; TH=$W/tmuxhome; mkdir -p "$ZH" "$TH"
scrub(){ env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u XDG_STATE_HOME TMUX_TMPDIR=/tmp "$@"; }
z(){ scrub HOME="$ZH" XDG_CONFIG_HOME="$ZH/config" ZZ_LOG_DIR="$W/zzlog" "$ZZ_BIN" --socket "$SOCK" "$@"; }
t(){ scrub HOME="$TH" XDG_CONFIG_HOME="$TH/config" "$TMUX_BIN" -L "zzp$TOK" -f /dev/null "$@"; }
cleanup(){ z kill-server >/dev/null 2>&1; t kill-server >/dev/null 2>&1; rm -rf -- "$W"; rm -f "$SOCK"; }
trap cleanup EXIT
z new-session -d -s cli -n w "ENV= PS1='\$ ' exec /bin/sh" >/dev/null 2>&1
t new-session -d -s cli -n w "ENV= PS1='\$ ' exec /bin/sh" >/dev/null 2>&1
sleep 1
for cmd in "choose-client -t %0" "choose-tree -t %0" "choose-buffer -t %0"; do
  echo "### $cmd"
  o=$(z $cmd 2>&1); echo "  zz   exit=$? out=[$o]"
  o=$(t $cmd 2>&1); echo "  pin  exit=$? out=[$o]"
  echo "  pin pane_in_mode after: $(t display-message -p -t %0 '#{pane_in_mode}' 2>&1)"
  echo "  zz  pane_in_mode after: $(z display-message -p -t %0 '#{pane_in_mode}' 2>&1)"
  t send-keys -t %0 -X cancel >/dev/null 2>&1
done
