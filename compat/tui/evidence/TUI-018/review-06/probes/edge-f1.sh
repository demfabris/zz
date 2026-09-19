#!/usr/bin/env bash
source "$(dirname "$0")/env.sh"
stop_both; start_both
for s in zz tmux; do
  echo "=== $s display twice"
  pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
  printf 'abc' | sidec $s display -I -t $pane \; display -I -t $pane \; set -g @after yes; echo "rc=$?"
  sidec $s show -gv @after; sample $s $pane 0
  sidec $s kill-pane -t $pane; sidec $s set -gu @after
  echo "=== $s split twice"
  printf 'xyz' | sidec $s split-window -I -d -t cs:win.0 \; split-window -I -d -t cs:win.0 \; set -g @after2 yes; echo "rc=$?"
  sidec $s show -gv @after2; sidec $s list-panes -t cs:win -F '#{pane_index} dead=#{pane_dead} #{pane_width}x#{pane_height}'
  for p in $(sidec $s list-panes -t cs:win -F '#{pane_id}' | tail -n +2); do sidec $s capture-pane -p -t $p -S 0 -E 0; sidec $s kill-pane -t $p; done
  echo "=== $s display then split then load-buffer spent"
  pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
  printf 'qq' | sidec $s display -I -t $pane \; load-buffer -b spentb - \; set -g @after3 yes; echo "rc=$?"
  sidec $s show -gv @after3; sidec $s kill-pane -t $pane
done
stop_both
