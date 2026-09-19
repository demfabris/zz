#!/usr/bin/env bash
source "$(dirname "$0")/env.sh"
stop_both; start_both
for s in zz tmux; do
  sidea $s
  for form in "-d -P" "-d"; do
    read -r -a f <<<"$form"
    for i in 1 2; do
      t0=$(date +%s%N)
      "${A[@]}" split-window "${f[@]}" -t cs:win.0 '' >/dev/null 2>&1 </dev/null
      t1=$(date +%s%N)
      echo "$s split-window $form run$i ms=$(( (t1-t0)/1000000 ))"
      p=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')
      sidec $s kill-pane -t $p 2>/dev/null
    done
  done
  t0=$(date +%s%N); "${A[@]}" display-message -p hi >/dev/null 2>&1 </dev/null; t1=$(date +%s%N)
  echo "$s display-message -p ms=$(( (t1-t0)/1000000 )) (client+daemon roundtrip baseline)"
done
stop_both
