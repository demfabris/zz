#!/usr/bin/env bash
source "$(dirname "$0")/env.sh"
stop_both; start_both
: > $P/wo
for s in zz tmux; do
  sidea $s
  sidec $s set -s 'command-alias[80]' 'unreadable=source-file - ; source-file - ; set -g @after yes'
  printf 'source-file -\nsource-file -\nset -g @after yes\n' > $P/f.conf
  r() { local label=$1; shift; "$@" >$P/out 2>$P/err; local rc=$?; echo "$label $s: rc=$rc stdout=[$(tr '\n' '|' <$P/out)] stderr=[$(tr '\n' '|' <$P/err)] @after=$(sidec $s show -gqv @after)"; sidec $s set -gu @after; }
  r "writeonly direct source;source" "${A[@]}" source-file - \; source-file - \; set -g @after yes 0>$P/wo
  r "writeonly direct source;source;display" "${A[@]}" source-file - \; source-file - \; display-message -p after-out 0>$P/wo
  r "writeonly alias" "${A[@]}" unreadable 0>$P/wo
  r "writeonly file" "${A[@]}" source-file $P/f.conf 0>$P/wo
  r "directory direct source;source;display" "${A[@]}" source-file - \; source-file - \; display-message -p after-out 0<$P
  r "readable source;display;source" "${A[@]}" source-file - \; display-message -p mid \; source-file - < <(printf 'set -g @a 1\n')
  r "readable source;display;source;display" "${A[@]}" source-file - \; display-message -p mid \; source-file - \; display-message -p end < <(printf 'set -g @a 1\n')
  r "readable load;display;load;display" "${A[@]}" load-buffer -b x1 - \; display-message -p mid \; load-buffer -b x2 - \; display-message -p end < <(printf abc)
done
stop_both
