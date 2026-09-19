. /tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/probe/env.sh
start_both
for s in zz tmux; do
  pane=$(sidec $s split-window -d -t =cs:win.0 -P -F '#{pane_id}' '')
  printf 'abc' | sidec $s display-message -I -t $pane
  sleep 0.3
  echo "$s: $(sidec $s capture-pane -p -t $pane | head -1) cursor=$(sidec $s display -p -t $pane '#{cursor_x}:#{cursor_y}') flag=$(sidec $s display -p -t $pane '#{cursor_flag}')"
  sidec $s kill-pane -t $pane
done
stop_both
