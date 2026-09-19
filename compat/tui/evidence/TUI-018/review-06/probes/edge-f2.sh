#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
run() {
  local s=$1 dest=$2 sig=$3 pane pid
  if [ $dest = display ]; then pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' ''); cmd=(display -I -t $pane); else pane=; cmd=(split-window -I -d -t cs:win.0); fi
  sidea $s
  rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo
  (exec "${A[@]}" "${cmd[@]}" \; set -g @after yes <$P/fifo >$P/out 2>$P/err 9>&-) &
  pid=$!
  if [ -z "$pane" ]; then for i in $(seq 100); do pane=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}'); [ -n "$pane" ] && break; sleep 0.03; done; fi
  printf '\033[31mRED\033[0m' >&9
  sleep 0.5
  echo "--- $s $dest $sig open: $(sample $s $pane 0 | tr '\n' ' ')"
  if [ $sig = EOF ]; then exec 9>&-; else kill -$sig $pid; fi
  for i in $(seq 40); do kill -0 $pid 2>/dev/null || break; sleep 0.05; done
  if kill -0 $pid 2>/dev/null; then echo "still running after $sig"; printf 'MORE' >&9; sleep 0.3; echo "--- after MORE: $(sample $s $pane 0 | tr '\n' ' ')"; exec 9>&-; sleep 0.3; fi
  wait $pid; echo "rc=$? out=[$(cat $P/out)] err=[$(cat $P/err)]"
  exec 9>&-
  sleep 0.2
  echo "--- after: $(sample $s $pane 0| tr '\n' ' ') @after=$(sidec $s show -gv @after 2>&1)"; sidec $s set -gu @after
  sidec $s kill-pane -t $pane
}
for sig in TERM HUP INT QUIT EOF; do for dest in display split; do for s in zz tmux; do run $s $dest $sig; done; done; done
stop_both
