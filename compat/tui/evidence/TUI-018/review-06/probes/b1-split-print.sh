#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo "HUNG"; kill -9 $SPID; }; wait $SPID; RC=$?; }
newpane() { sidec $1 list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}'; }
for form in "split-window -I -d -t cs:win.0" "split-window -I -d -P -t cs:win.0" "split-window -I -d -P -F #{pane_id} -t cs:win.0"; do
  for end in EOF TERM@0.8 TERM@3; do
    for s in zz tmux; do
      read -r -a cmd <<<"$form"
      open_stream $s "${cmd[@]}" \; set -g @after yes
      printf '\033[31mRED\033[0m' >&9
      sleep 0.8
      np=$(newpane $s)
      open="$(sample $s $np 0 | tr '\n' ' ') stdout=[$(tr '\n' '|' <$P/out)]"
      case $end in
        EOF) finish ;;
        TERM@0.8) kill -TERM $SPID; finish ;;
        TERM@3) sleep 2.2; kill -TERM $SPID; finish ;;
      esac
      echo "[$form] $end $s open@0.8s: $open || rc=$RC stdout=[$(tr '\n' '|' <$P/out)] stderr=[$(tr '\n' '|' <$P/err)] after=$(sidec $s show -gqv @after) final: $(sample $s $np 0 | tr '\n' ' ')"
      sidec $s set -gu @after; sidec $s kill-pane -t $np
    done
  done
done
for s in zz tmux; do
  open_stream $s split-window -I -d -P -t cs:win.0
  t0=$(date +%s%N); printf 'GREEN' >&9
  np=""; for i in $(seq 100); do np=$(newpane $s); [ -n "$np" ] && break; sleep 0.02; done
  for i in $(seq 100); do sidec $s capture-pane -p -t $np | grep -q GREEN && break; sleep 0.05; done
  t1=$(date +%s%N); finish
  echo "$s split-window -I -P: GREEN first visible after ~$(( (t1-t0)/100000000 ))00 ms"
  sidec $s kill-pane -t $np
  t0=$(date +%s%N); sidec $s split-window -d -P -t cs:win.0 '' >/dev/null </dev/null; t1=$(date +%s%N)
  echo "$s split-window -d -P '' (no -I) took ~$(( (t1-t0)/100000000 ))00 ms"
  for p in $(sidec $s list-panes -t cs:win -F '#{pane_id}' | tail -n +2); do sidec $s kill-pane -t $p; done
done
stop_both
