#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo HUNG; kill -9 $SPID; }; wait $SPID; RC=$?; }
for s in zz tmux; do
  open_stream $s split-window -I -d -P -t cs:win.0
  printf 'RED' >&9
  for t in 1 3 5 7; do
    sleep 2
    np=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')
    echo "$s at~${t}s screen=[$(sidec $s capture-pane -p -t $np -S 0 -E 0 2>&1)] out=[$(tr '\n' '|' <$P/out)]"
  done
  finish
  echo "$s after EOF rc=$RC screen=[$(sidec $s capture-pane -p -t $np -S 0 -E 0)] out=[$(tr '\n' '|' <$P/out)]"
  sidec $s kill-pane -t $np
done
stop_both
