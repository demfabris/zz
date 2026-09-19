#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { # side, cmd...
  local s=$1; shift; sidea $s
  rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo
  (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) &
  SPID=$!
}
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo "HUNG, killing"; kill -9 $SPID; }; wait $SPID; RC=$?; }
echo "##### E3 stall mid-escape then close"
for s in zz tmux; do
  pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
  open_stream $s display -I -t $pane \; set -g @after yes
  printf 'AB\033[3' >&9; sleep 0.4
  echo "$s open: $(sample $s $pane 0 | tr '\n' ' ')"
  finish
  echo "$s rc=$RC out=[$(cat $P/out)] err=[$(cat $P/err)] after=$(sidec $s show -gv @after 2>&1)"; echo "$s final: $(sample $s $pane 0 | tr '\n' ' ')"
  sidec $s set -gu @after; sidec $s kill-pane -t $pane
done
echo "##### E3b stall mid-UTF8 then close"
for s in zz tmux; do
  pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
  open_stream $s display -I -t $pane
  printf 'AB\303' >&9; sleep 0.4
  finish
  echo "$s rc=$RC final: $(sample $s $pane 0 | tr '\n' ' ')"
  sidec $s kill-pane -t $pane
done
for variant in more close; do
echo "##### E5 pane killed mid-stream then $variant"
for s in zz tmux; do
  pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
  open_stream $s display -I -t $pane \; set -g @after yes \; display -p tail-out
  printf 'abc' >&9; sleep 0.3
  sidec $s kill-pane -t $pane
  sleep 0.2
  if [ $variant = more ]; then printf 'def' >&9; sleep 0.5; fi
  kill -0 $SPID 2>/dev/null && echo "$s still running before close" || echo "$s exited before close"
  finish
  echo "$s rc=$RC out=[$(tr '\n' '|' <$P/out)] err=[$(tr '\n' '|' <$P/err)] after=$(sidec $s show -gv @after 2>&1) panes=$(sidec $s list-panes -t cs:win | wc -l)"
  sidec $s set -gu @after
done
done
echo "##### E5c split -I pane killed mid-stream then more"
for s in zz tmux; do
  open_stream $s split-window -I -d -t cs:win.0 \; set -g @after yes \; display -p tail-out
  sleep 0.3; printf 'abc' >&9; sleep 0.3
  pane=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')
  sidec $s kill-pane -t $pane; sleep 0.2
  printf 'def' >&9; sleep 0.5
  kill -0 $SPID 2>/dev/null && echo "$s still running before close" || echo "$s exited before close"
  finish
  echo "$s rc=$RC out=[$(tr '\n' '|' <$P/out)] err=[$(tr '\n' '|' <$P/err)] after=$(sidec $s show -gv @after 2>&1) panes=$(sidec $s list-panes -t cs:win | wc -l)"
  sidec $s set -gu @after
done
stop_both
