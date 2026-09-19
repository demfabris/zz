#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
awk 'BEGIN { for (i = 1; i <= 180000; i++) printf "%06d ", i }' > $P/big.bin
wc -c < $P/big.bin
for form in "split-window -I -d -t cs:win.0" "split-window -I -d -P -t cs:win.0"; do
  for s in zz tmux; do
    sidea $s
    read -r -a cmd <<<"$form"
    ( exec "${A[@]}" "${cmd[@]}" < $P/big.bin > $P/out 2> $P/err ) ; rc=$?
    np=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')
    echo "[$form] $s rc=$rc out=[$(tr '\n' '|' <$P/out)] err=[$(tr '\n' '|' <$P/err)] last=[$(sidec $s capture-pane -p -t $np -S 0 -E 23 2>/dev/null | grep -v '^$' | tail -1)] cursor=$(sidec $s display -p -t $np '#{cursor_x}:#{cursor_y}' 2>&1)"
    sidec $s kill-pane -t $np 2>/dev/null
  done
done
stop_both
