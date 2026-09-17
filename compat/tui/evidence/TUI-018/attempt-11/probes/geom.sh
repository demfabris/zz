. /tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/probe/env.sh
start_both
for s in zz tmux; do
  pane=$(sidec $s split-window -d -t =cs:win.0 -P -F '#{pane_id}' '')
  seq 1 30 | sidec $s display-message -I -t $pane
  sleep 0.5
  echo "$s size=$(sidec $s display -p -t $pane '#{pane_width}x#{pane_height} cursor=#{cursor_x}:#{cursor_y} history=#{history_size}') cap=$(sidec $s capture-pane -p -t $pane | tr '\n' ' ')"
  sidec $s kill-pane -t $pane
done
stop_both
