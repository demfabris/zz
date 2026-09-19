#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo "HUNG, killing"; kill -9 $SPID; }; wait $SPID; RC=$?; }
s=zz
echo "## pane killed mid-stream then more"
pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
open_stream $s display -I -t $pane \; set -g @after yes \; display -p tail-out
printf 'abc' >&9; sleep 0.3; sidec $s kill-pane -t $pane; sleep 0.2; printf 'def' >&9; sleep 0.5
finish; echo "rc=$RC out=[$(tr '\n' '|' <$P/out)] err=[$(tr '\n' '|' <$P/err)] after=$(sidec $s show -gv @after 2>&1)"; sidec $s set -gu @after
echo "## split -P open"
open_stream $s split-window -I -d -P -t cs:win.0
sleep 0.4; printf 'GREEN' >&9; sleep 0.4
np=$(sidec $s list-panes -t cs:win -F '#{pane_index} #{pane_id}' | awk '$1==1{print $2}')
o="$(sample $s $np 0 | tr '\n' ' ') out=[$(tr '\n' '|' <$P/out)]"; kill -TERM $SPID
finish; echo "open: $o | TERM rc=$RC out=[$(tr '\n' '|' <$P/out)] final: $(sample $s $np 0 | tr '\n' ' ')"; sidec $s kill-pane -t $np
echo "## display -I SIGHUP / SIGINT"
for sig in HUP INT; do
pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
open_stream $s display -I -t $pane \; set -g @after yes
printf 'abc' >&9; sleep 0.4; kill -$sig $SPID; sleep 0.3
finish; echo "$sig rc=$RC after=$(sidec $s show -gv @after 2>&1) pane=$(sample $s $pane 0 | tr '\n' ' ')"; sidec $s set -gu @after; sidec $s kill-pane -t $pane
done
echo "## run-shell sleep SIGHUP / SIGINT / TERM (generic waiting client)"
for sig in HUP INT TERM; do
open_stream $s run-shell 'sleep 2' \; set -g @after yes
sleep 0.4; kill -$sig $SPID; sleep 2.2
finish; echo "$sig rc=$RC after=$(sidec $s show -gv @after 2>&1)"; sidec $s set -gu @after
done
echo "## writeonly two readers"
: > $P/wo; sidea $s
"${A[@]}" source-file - \; source-file - \; display -p after-out 0>$P/wo >$P/out 2>$P/err; echo "ss-d rc=$? out=[$(tr '\n' '|' <$P/out)] err=[$(tr '\n' '|' <$P/err)]"
stop_both
