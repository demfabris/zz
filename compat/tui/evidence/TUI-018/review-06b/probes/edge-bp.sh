#!/usr/bin/env bash
source "$(dirname "$0")/env.sh"
stop_both; start_both
dpid=$(pgrep -f "zz_cli.*--socket $ZS.*daemon" | head -1)
awk -v n=$((64*1024*1024/8)) 'BEGIN { for (i = 1; i <= n; i++) printf "%07d\n", i % 10000000 }' > $P/big64
tpid=$(pgrep -f "tmux.*-L $TL" | head -1)
echo "daemon rss_kb_before=$(awk '/VmRSS/{print $2}' /proc/$dpid/status) tmux-server rss_kb=$(awk '/VmRSS/{print $2}' /proc/$tpid/status)"
for s in zz tmux; do
  sidea $s
  pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
  start=$(date +%s%N)
  ( exec "${A[@]}" display -I -t $pane < <(cat $P/big64) ) &
  cpid=$!
  sleep 1
  wpid=$(pgrep -P $$ -x cat | head -1)
  hwm=0; samples=""
  while kill -0 $cpid 2>/dev/null; do
    h=$(awk '/VmHWM/{print $2}' /proc/$cpid/status 2>/dev/null); [ -n "$h" ] && hwm=$h
    [ -z "$samples" ] && samples="writer_wchan=$(cat /proc/$wpid/wchan 2>/dev/null) client_wchan=$(cat /proc/$cpid/wchan 2>/dev/null)"
    sleep 0.2
  done
  wait $cpid; rc=$?
  end=$(date +%s%N)
  echo "$s 64MiB rc=$rc ms=$(( (end-start)/1000000 )) client_VmHWM_kb=$hwm $samples last=$(sidec $s capture-pane -p -t $pane | grep -v '^$' | tail -1)"
  [ $s = zz ] && echo "daemon rss_kb_after=$(awk '/VmRSS/{print $2}' /proc/$dpid/status) hwm=$(awk '/VmHWM/{print $2}' /proc/$dpid/status)"
  [ $s = tmux ] && echo "tmux-server rss_kb_after=$(awk '/VmRSS/{print $2}' /proc/$tpid/status)"
  sidec $s kill-pane -t $pane
done
for s in zz tmux; do sidea $s; ( exec "${A[@]}" list-sessions < /dev/null ) & cpid=$!; wait $cpid; done >/dev/null
stop_both
