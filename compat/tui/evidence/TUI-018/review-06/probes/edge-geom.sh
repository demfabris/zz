#!/usr/bin/env bash
source "$(dirname "$0")/env.sh"
stop_both; start_both
for s in zz tmux; do
  sidec $s new-session -d -s big -x 132 -y 40 "ENV= PS1='\$ ' exec /bin/sh"
  pane=$(sidec $s split-window -h -d -t big:0.0 -P -F '#{pane_id}' '')
  awk 'BEGIN{for(i=0;i<150;i++) printf "%d", i%10}' | sidec $s display -I -t $pane
  echo "$s: $(sample $s $pane 2 | tr '\n' '|')"
  printf 'more\r\n' | sidec $s split-window -I -d -l 7 -t big:0.0
  np=$(sidec $s list-panes -t big:0 -F '#{pane_index} #{pane_id} #{pane_height}' | tr '\n' ,)
  echo "$s panes: $np"
  sidec $s kill-session -t big
done
stop_both
