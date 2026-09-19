#!/usr/bin/env bash
source "$(dirname "$0")/env.sh"
stop_both; start_both
python3 - "$P" <<'PY'
import sys
p=sys.argv[1]
out=bytearray()
line=b'x'*78+b'\r\n'
while len(out)+80<=16320: out+=line
out+=b'y'*(16383-len(out))
out+='é'.encode()
out+=b'\r\n'
while len(out)+80<=32720: out+=line
out+=b'z'*(32766-len(out))
out+=b'\x1b[31mRED\x1b[0m'
out+=b'\r\n'
while len(out)+80<=49100: out+=line
pad=49152-len(out)-3
out+=b'w'*pad
out+='中'.encode()
out+=b'\r\nEND'
assert out[16383:16385]=='é'.encode(), out[16380:16390]
assert out[32766:32771]==b'\x1b[31m'
open(p+'/split.bin','wb').write(out)
print(len(out))
PY
for mode in cat chunked; do
 for s in zz tmux; do
  sidea $s
  pane=$(sidec $s split-window -d -t cs:win.0 -P -F '#{pane_id}' '')
  sidec $s resize-pane -Z -t $pane
  if [ $mode = cat ]; then cat $P/split.bin | "${A[@]}" display -I -t $pane; rc=$?
  else (dd if=$P/split.bin bs=16384 count=1 2>/dev/null; sleep 0.3; dd if=$P/split.bin bs=16384 skip=1 count=1 2>/dev/null; sleep 0.3; dd if=$P/split.bin bs=16384 skip=2 2>/dev/null) | "${A[@]}" display -I -t $pane; rc=$?; fi
  sleep 0.3
  sidec $s capture-pane -e -p -S - -t $pane | cat -v > $P/$s.cap
  sidec $s display -p -t $pane 'cursor=#{cursor_x}:#{cursor_y} size=#{pane_width}x#{pane_height} zoomed=#{window_zoomed_flag}' >> $P/$s.cap
  echo "rc=$rc" >> $P/$s.cap
  sidec $s kill-pane -t $pane
 done
 if cmp -s $P/zz.cap $P/tmux.cap; then echo "$mode SAME"; tail -8 $P/zz.cap; else echo "$mode DIFF"; diff $P/tmux.cap $P/zz.cap; fi
done
stop_both
