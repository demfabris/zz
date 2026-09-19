. /tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/probe/env.sh
start_both
for body in "set -g @b yes ; run-shell -t %99 'echo x' ; set -g @tail yes" "set -g @b yes ; run-shell 'exit 3' ; set -g @tail yes" "set -g @b yes ; run-shell -d bogus 'echo x' ; set -g @tail yes" "set -g @b yes ; run-shell 'echo out' ; set -g @tail yes"; do
for s in zz tmux; do
  if [ $s = zz ]; then B=(env -u TMUX HOME=$P/zzhome XDG_CONFIG_HOME=$P/zzhome/config $ZZB --socket $ZS); else B=(env -u TMUX TMUX_TMPDIR=/tmp HOME=$P/tmhome $TMB -L zzp6); fi
  sidec $s set -s 'command-alias[90]' "zzp=$body"
  "${B[@]}" -C attach-session -f no-output -t =cs < <(printf 'zzp\n'; sleep 1.5) > $P/$s.ctl 2>$P/$s.err
  echo "rc=$? tail=$(sidec $s show -gqv @tail)" >> $P/$s.ctl
  sed -i -E 's/^(%begin|%end|%error) [0-9]+ [0-9]+ /\1 T I /' $P/$s.ctl
  sidec $s set -gu @b; sidec $s set -gu @tail
done
cat $P/tmux.ctl; if cmp -s $P/zz.ctl $P/tmux.ctl; then echo "SAME [$body]"; else echo "DIFF [$body]"; diff $P/tmux.ctl $P/zz.ctl; fi
done
stop_both
