. /tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/probe/env.sh
start_both
: > $P/wo
for kind in wo dir; do
for s in zz tmux; do
  pane=$(sidec $s split-window -d -t =cs:win.0 -P -F '#{pane_id}' '')
  for reader in "source-file -" "load-buffer -b pb -" "display-message -I -t $pane" "split-window -I -d -t =cs:win.0"; do
    if [ $kind = wo ]; then
      sidec $s $reader \; display-message -p after \; set -g @tail yes 0>$P/wo >$P/$s.out 2>$P/$s.err
    else
      sidec $s $reader \; display-message -p after \; set -g @tail yes 0</tmp >$P/$s.out 2>$P/$s.err
    fi
    rc=$?
    echo "$kind $s [${reader%% -t*}] rc=$rc out=$(cat $P/$s.out|tr '\n' '|') err=$(cat $P/$s.err|tr '\n' '|') tail=$(sidec $s show -gqv @tail) panes=$(sidec $s list-panes -t =cs:win -F x | wc -l)"
    sidec $s set -gu @tail
  done
  sidec $s kill-pane -a -t =cs:win.0
done
done
stop_both
