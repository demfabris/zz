#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo "HUNG"; kill -9 $SPID; }; wait $SPID; RC=$?; }
for dest in display split; do
for variant in more eof; do
for s in zz tmux; do
  if [ $dest = display ]; then
    pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
    open_stream $s display-message -I -t $pane \; set -g @after yes \; display-message -p tail-out
  else
    open_stream $s split-window -I -d -t cs:win.0 \; set -g @after yes \; display-message -p tail-out
    sleep 0.5
    pane=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')
  fi
  printf 'abc' >&9; sleep 0.5
  sidec $s kill-pane -t $pane; sleep 0.3
  [ $variant = more ] && { printf 'def' >&9; sleep 0.5; }
  alive=$(kill -0 $SPID 2>/dev/null && echo running || echo exited)
  finish
  echo "$dest $variant $s: client=$alive rc=$RC out=[$(tr '\n' '|' <$P/out)] err=[$(tr '\n' '|' <$P/err)] @after=[$(sidec $s show -gv @after 2>&1)] panes=$(sidec $s list-panes -t cs:win | wc -l)"
  sidec $s set -gu @after 2>/dev/null
done
done
done
stop_both
