#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo "HUNG, killing"; kill -9 $SPID; }; wait $SPID; RC=$?; }
v() {
  local name=$1 end=$2; shift 2
  for s in zz tmux; do
    open_stream $s "$@"
    sleep 0.4; printf 'GREEN' >&9; sleep 0.4
    np=$(sidec $s list-panes -a -F '#{pane_id} #{pane_current_command}#{?pane_pid,P,N}' | awk '$2=="N"{print $1}' | tail -1)
    [ -z "$np" ] && np=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')
    o="$(sample $s $np 0 | tr '\n' ' ') out=[$(tr '\n' '|' <$P/out)]"
    if [ $end = TERM ]; then kill -TERM $SPID; fi
    finish
    echo "$name $s open: $o | rc=$RC final out=[$(tr '\n' '|' <$P/out)] err=[$(tr '\n' '|' <$P/err)] final: $(sample $s $np 0 | tr '\n' ' ')"
    for p in $(sidec $s list-panes -t cs:win -F '#{pane_id}' | tail -n +2); do sidec $s kill-pane -t $p; done
  done
}
v P EOF split-window -I -d -P -t cs:win.0
v PF EOF split-window -I -d -P -F '#{pane_id}' -t cs:win.0
v P-TERM TERM split-window -I -d -P -t cs:win.0
v h EOF split-window -I -d -h -t cs:win.0
v b EOF split-window -I -d -b -t cs:win.0
v f EOF split-window -I -d -f -t cs:win.0
v l EOF split-window -I -d -l 5 -t cs:win.0
v Z EOF split-window -I -Z -t cs:win.0
v e EOF split-window -I -d -e FOO=bar -t cs:win.0
v c EOF split-window -I -d -c /tmp -t cs:win.0
v display-then-split EOF display -p x \; split-window -I -d -t cs:win.0
stop_both
