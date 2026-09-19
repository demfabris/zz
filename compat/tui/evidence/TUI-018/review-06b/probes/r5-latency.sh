#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && kill -9 $SPID; wait $SPID; RC=$?; }
for rep in 1 2; do
for form in "split-window -I -d -t cs:win.0" "split-window -I -d -P -t cs:win.0" "display"; do
  for s in zz tmux; do
    if [ "$form" = display ]; then
      pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
      open_stream $s display-message -I -t $pane
      t0=$(date +%s%N)
      printf 'GREEN' >&9
      np=$pane
    else
      read -r -a cmd <<<"$form"
      open_stream $s "${cmd[@]}"
      t0=$(date +%s%N)
      printf 'GREEN' >&9
      np=""
      for i in $(seq 60); do np=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}'); [ -n "$np" ] && break; done
    fi
    hit=""
    for i in $(seq 60); do
      if sidec $s capture-pane -p -t $np 2>/dev/null | grep -q GREEN; then hit=$(( ($(date +%s%N)-t0)/1000000 )); break; fi
    done
    echo "rep$rep [$form] $s first_visible_ms=${hit:-TIMEOUT} probes=$i"
    finish
    sidec $s kill-pane -t $np 2>/dev/null
  done
done
done
stop_both
