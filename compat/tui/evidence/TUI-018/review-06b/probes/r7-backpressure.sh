#!/usr/bin/env bash
source "$(dirname "$0")/env.sh"
stop_both; start_both
dpid=$(pgrep -f "zz_cli.*--socket $ZS.*daemon" | head -1)
tpid=$(pgrep -f "tmux.*-L $TL" | head -1)
echo "before: daemon_rss_kb=$(awk '/VmRSS/{print $2}' /proc/$dpid/status) tmux_rss_kb=$(awk '/VmRSS/{print $2}' /proc/$tpid/status)"
for s in zz tmux; do
  sidea $s
  pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
  start=$(date +%s%N)
  ( exec "${A[@]}" display-message -I -t $pane < <(awk 'BEGIN { for (i = 1; i <= 1048576; i++) printf "%07d\n", i % 10000000 }') ) &
  cpid=$!
  hwm=0
  while kill -0 $cpid 2>/dev/null; do
    h=$(awk '/VmHWM/{print $2}' /proc/$cpid/status 2>/dev/null); [ -n "$h" ] && hwm=$h
    sleep 0.2
  done
  wait $cpid; rc=$?
  end=$(date +%s%N)
  echo "$s 8MiB rc=$rc ms=$(( (end-start)/1000000 )) client_VmHWM_kb=$hwm last=[$(sidec $s capture-pane -p -t $pane | grep -v '^$' | tail -1)]"
  [ $s = zz ] && echo "  daemon_rss_kb_after=$(awk '/VmRSS/{print $2}' /proc/$dpid/status) daemon_hwm_kb=$(awk '/VmHWM/{print $2}' /proc/$dpid/status)"
  [ $s = tmux ] && echo "  tmux_rss_kb_after=$(awk '/VmRSS/{print $2}' /proc/$tpid/status) tmux_hwm_kb=$(awk '/VmHWM/{print $2}' /proc/$tpid/status)"
  sidec $s kill-pane -t $pane
done
stop_both
