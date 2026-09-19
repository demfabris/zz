. /tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/probe/env.sh
start_both
printf 'HELLO\n' > $P/in.txt
for s in zz tmux; do
  pane=$(sidec $s split-window -d -t =cs:win.0 -P -F '#{pane_id}' '')
  sidec $s display-message -I -t $pane \; display-message -I -t $pane \; display-message -p after \; set -g @tail yes < $P/in.txt > $P/$s.out 2> $P/$s.err
  echo "$s rc=$? out=$(cat $P/$s.out) err=$(cat $P/$s.err) tail=$(sidec $s show -gqv @tail) cap=$(sidec $s capture-pane -p -t $pane | head -2 | tr '\n' '|')"
  sidec $s set -gu @tail
  sidec $s kill-pane -t $pane
  sidec $s split-window -I -d -t =cs:win.0 \; split-window -I -d -t =cs:win.0 \; display-message -p after \; set -g @tail yes < $P/in.txt > $P/$s.out 2> $P/$s.err
  echo "$s split rc=$? out=$(cat $P/$s.out) err=$(cat $P/$s.err) tail=$(sidec $s show -gqv @tail) panes=$(sidec $s list-panes -F '#{pane_index}:#{?pane_pid,p,e}' | tr '\n' ' ')"
  sidec $s set -gu @tail
  sidec $s kill-pane -a -t =cs:win.0
done
stop_both
