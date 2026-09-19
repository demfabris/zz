#!/usr/bin/env bash
set -m
source "$(dirname "$0")/env.sh"
stop_both; start_both
open_stream() { local s=$1; shift; sidea $s; rm -f $P/fifo; mkfifo $P/fifo; exec 9<>$P/fifo; (exec "${A[@]}" "$@" <$P/fifo >$P/out 2>$P/err 9>&-) & SPID=$!; }
finish() { exec 9>&-; for i in $(seq 100); do kill -0 $SPID 2>/dev/null || break; sleep 0.05; done; kill -0 $SPID 2>/dev/null && { echo "HUNG, killing"; kill -9 $SPID; }; wait $SPID; RC=$?; }
for s in tmux zz; do
for sig in HUP INT TERM; do
open_stream $s run-shell 'sleep 2' \; set -g @after yes
sleep 0.4; kill -$sig $SPID; sleep 2.2
finish; echo "$s run-shell $sig rc=$RC after=$(sidec $s show -gv @after 2>&1)"; sidec $s set -gu @after
done
for sig in HUP INT; do
open_stream $s source-file - \; set -g @after yes
printf 'set -g @x 1\n' >&9; sleep 0.4; kill -$sig $SPID; sleep 0.3
finish; echo "$s source-file $sig rc=$RC after=$(sidec $s show -gv @after 2>&1) x=$(sidec $s show -gv @x 2>&1)"; sidec $s set -gu @after; sidec $s set -gu @x
done
done
stop_both
