. /tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/probe/env.sh
start_both
for inv in direct alias file; do
for pos in before after; do
for s in zz tmux; do
  rm -f $P/ready
  if [ $pos = before ]; then
    body="set -g @b yes ; run-shell 'printf ready > $P/ready; sleep 1' ; source-file - ; display-message -p after ; set -g @tail yes"
  else
    body="set -g @b yes ; source-file - ; run-shell 'printf ready > $P/ready; sleep 1' ; display-message -p after ; set -g @tail yes"
  fi
  if [ $s = zz ]; then B=(env -u TMUX HOME=$P/zzhome XDG_CONFIG_HOME=$P/zzhome/config $ZZB --socket $ZS); else B=(env -u TMUX TMUX_TMPDIR=/tmp HOME=$P/tmhome $TMB -L zzp6); fi
  case $inv in
  direct) eval "cmd=($(printf '%s' "$body" | sed "s/ ; / \\\; /g"))" ;;
  alias) sidec $s set -s 'command-alias[90]' "zzp=$body"; cmd=(zzp) ;;
  file) printf '%s\n' "$body" > $P/r.conf; cmd=(source-file $P/r.conf) ;;
  esac
  ("${B[@]}" -C "${cmd[@]}" </dev/null > $P/$s.ctl 2>$P/$s.err) &
  pid=$!
  for i in $(seq 100); do [ -f $P/ready ] && break; sleep 0.03; done
  sleep 0.05
  kill -TERM $pid
  wait $pid; rc=$?
  sed -E 's/^(%begin|%end|%error) [0-9]+ [0-9]+ /\1 T I /' $P/$s.ctl | grep -v '^%output\|^%window\|^%session\|^%sessions\|^%layout\|^%unlinked\|^%client' > $P/$s.norm
  echo "rc=$rc tail=$(sidec $s show -gqv @tail) err=$(cat $P/$s.err)" >> $P/$s.norm
  sleep 1.2
  sidec $s set -gu @b; sidec $s set -gu @tail
done
if cmp -s $P/zz.norm $P/tmux.norm; then echo "SAME $inv $pos"; else echo "DIFF $inv $pos"; diff $P/tmux.norm $P/zz.norm; fi
done
done
stop_both
