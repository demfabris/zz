#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; wait $SPID; RC=$?; }
for s in zz tmux; do
  pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
  open_stream $s display -I -t $pane \; set -g @after yes
  printf 'abc' >&9; sleep 0.4; kill -KILL $SPID; sleep 0.5; finish
  echo "$s display KILL rc=$RC after=$(sidec $s show -gv @after 2>&1) pane=$(sample $s $pane 0 | tr '\n' ' ') clients=$(sidec $s list-clients | wc -l)"
  sidec $s set -gu @after; sidec $s kill-pane -t $pane
  open_stream $s split-window -I -d -t cs:win.0 \; set -g @after yes
  printf 'abc' >&9; sleep 0.4; kill -KILL $SPID; sleep 0.5; finish
  echo "$s split KILL rc=$RC after=$(sidec $s show -gv @after 2>&1) panes=$(sidec $s list-panes -t cs:win | wc -l)"
  sidec $s set -gu @after; for p in $(sidec $s list-panes -t cs:win -F '#{pane_id}' | tail -n +2); do sidec $s kill-pane -t $p; done
  open_stream $s source-file - \; set -g @after yes
  printf 'set -g @x 1\n' >&9; sleep 0.4; kill -KILL $SPID; sleep 0.5; finish
  echo "$s source KILL rc=$RC after=$(sidec $s show -gv @after 2>&1) x=$(sidec $s show -gv @x 2>&1)"
  sidec $s set -gu @after; sidec $s set -gu @x
done
stop_both
