#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo "HUNG, killing"; kill -9 $SPID; }; wait $SPID; RC=$?; }
v() {
  local name=$1 zoom=$2; shift 2
  for s in zz tmux; do
    sidec $s split-window -d -t cs:win.0 'sleep 100'
    [ $zoom = 1 ] && sidec $s resize-pane -Z -t cs:win.0
    open_stream $s "$@"
    sleep 0.4; printf 'GREEN' >&9; sleep 0.4
    np=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')
    o="$(sample $s $np 0 | tr '\n' ' ') out=[$(tr '\n' '|' <$P/out)]"
    finish
    echo "$name $s open: $o | rc=$RC final out=[$(tr '\n' '|' <$P/out)]"
    for p in $(sidec $s list-panes -t cs:win -F '#{pane_id}' | tail -n +2); do sidec $s kill-pane -t $p; done
  done
}
v nozoom-d 0 split-window -I -d -t cs:win.0
v nozoom-nod 0 split-window -I -t cs:win.0
v zoom-d 1 split-window -I -d -t cs:win.0
v zoom-nod 1 split-window -I -t cs:win.0
v nozoom-d-P 0 split-window -I -d -P -t cs:win.0
v nozoom-d-tail 0 split-window -I -d -t cs:win.0 \; display -p tail
stop_both
