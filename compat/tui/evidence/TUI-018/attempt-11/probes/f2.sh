. /tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/probe/env.sh
start_both
for dest in display split; do
for s in zz tmux; do
  if [ $s = zz ]; then B=(env -u TMUX HOME=$P/zzhome XDG_CONFIG_HOME=$P/zzhome/config $ZZB --socket $ZS); else B=(env -u TMUX TMUX_TMPDIR=/tmp HOME=$P/tmhome $TMB -L zzp6); fi
  if [ $dest = display ]; then
    pane=$(sidec $s split-window -d -t =cs:win.0 -P -F '#{pane_id}' '')
    cmd=(display-message -I -t $pane)
  else
    cmd=(split-window -I -d -t =cs:win.0)
    pane=
  fi
  rm -f $P/fifo; mkfifo $P/fifo
  exec 9<>$P/fifo
  ("${B[@]}" "${cmd[@]}" <$P/fifo 9>&- >$P/$s.out 2>$P/$s.err) &
  pid=$!
  printf '\033[31mRED\033[0m' >&9
  sleep 0.5
  [ -n "$pane" ] || pane=$(sidec $s list-panes -t =cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')
  echo "$s $dest open: $(sidec $s capture-pane -e -p -t $pane | head -1 | cat -v) cursor=$(sidec $s display -p -t $pane '#{cursor_x}:#{cursor_y}')"
  printf '\303' >&9; sleep 0.3; printf '\251\033[3' >&9; sleep 0.3; printf '2mGREEN\033[0m' >&9; sleep 0.3
  echo "$s $dest split: $(sidec $s capture-pane -e -p -t $pane | head -1 | cat -v) cursor=$(sidec $s display -p -t $pane '#{cursor_x}:#{cursor_y}')"
  kill -TERM $pid
  wait $pid; echo "$s rc=$? err=$(cat $P/$s.err)"
  exec 9>&-
  sleep 0.3
  echo "$s $dest after: $(sidec $s capture-pane -e -p -t $pane | head -1 | cat -v) cursor=$(sidec $s display -p -t $pane '#{cursor_x}:#{cursor_y}')"
  sidec $s kill-pane -t $pane
done
done
stop_both
