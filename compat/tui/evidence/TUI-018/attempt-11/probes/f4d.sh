. /tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/probe/env.sh
start_both
for s in zz tmux; do
  if [ $s = zz ]; then B=(env -u TMUX HOME=$P/zzhome XDG_CONFIG_HOME=$P/zzhome/config $ZZB --socket $ZS); else B=(env -u TMUX TMUX_TMPDIR=/tmp HOME=$P/tmhome $TMB -L zzp6); fi
  cmd=(set -g @b yes \; run-shell "sleep 1" \; display-message -p after \; set -g @tail yes)
  "${B[@]}" -C "${cmd[@]}" </dev/null > $P/$s.ctl 2>$P/$s.err
  rc=$?
  sleep 1.5
  echo "== $s rc=$rc tail=$(sidec $s show -gqv @tail)"; sed -E 's/^(%begin|%end|%error) [0-9]+ [0-9]+ /\1 T I /' $P/$s.ctl | grep -v '^%output'
  sidec $s set -gu @b; sidec $s set -gu @tail
done
stop_both
