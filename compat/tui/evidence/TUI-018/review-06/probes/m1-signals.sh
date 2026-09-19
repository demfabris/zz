#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec env --default-signal=HUP,INT,QUIT "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo "HUNG"; kill -9 $SPID; }; wait $SPID; RC=$?; }
for sig in TERM HUP INT QUIT; do
for form in display source run-shell; do
for s in zz tmux; do
  case $form in
    display) pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' ''); open_stream $s display-message -I -t $pane \; set -g @after yes; printf 'abc' >&9 ;;
    source) open_stream $s source-file - \; set -g @after yes; printf 'set -g @x 1\n' >&9 ;;
    run-shell) open_stream $s run-shell 'sleep 1' \; set -g @after yes ;;
  esac
  sleep 0.4; kill -$sig $SPID; sleep 0.3
  alive=$(kill -0 $SPID 2>/dev/null && echo alive || echo gone)
  if [ $alive = alive ] && [ $form = display ]; then printf 'MORE' >&9; sleep 0.3; fi
  [ $form = run-shell ] && sleep 1
  finish
  extra=""; [ $form = display ] && extra="pane=$(sidec $s capture-pane -p -t $pane -S 0 -E 0)"; [ $form = source ] && extra="@x=$(sidec $s show -gqv @x)"
  echo "$sig $form $s: after-signal=$alive rc=$RC @after=$(sidec $s show -gqv @after) $extra"
  sidec $s set -gu @after; sidec $s set -gu @x; [ $form = display ] && sidec $s kill-pane -t $pane
done
done
done
stop_both
