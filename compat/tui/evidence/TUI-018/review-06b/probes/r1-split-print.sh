#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo "HUNG"; kill -9 $SPID; }; wait $SPID; RC=$?; }
newpane() { sidec $1 list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}'; }
for form in "split-window -I -d -t cs:win.0" "split-window -I -d -P -t cs:win.0"; do
  for end in EOF TERM; do
    for s in zz tmux; do
      read -r -a cmd <<<"$form"
      open_stream $s "${cmd[@]}"
      printf '\033[31mRED\033[0m' >&9
      sleep 1.2
      np=$(newpane $s)
      open="$(sample $s $np 0 | tr '\n' ' ')out=[$(tr '\n' '|' <$P/out)]"
      [ $end = TERM ] && kill -TERM $SPID
      finish
      echo "[$form] $end $s OPEN: $open || rc=$RC out=[$(tr '\n' '|' <$P/out)] err=[$(tr '\n' '|' <$P/err)] FINAL: $(sample $s $np 0 | tr '\n' ' ')"
      sidec $s kill-pane -t $np
    done
  done
done
stop_both
