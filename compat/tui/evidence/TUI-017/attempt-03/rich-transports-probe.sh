set -u
PIN="${ZZ_COMPAT_TMUX:-$PWD/compat/.cache/tmux-src/tmux}"
ZZ="${ZZ_BIN:-$PWD/target/debug/zz}"
D=/tmp/zzrich.$$; rm -rf "$D"; mkdir -p "$D/h/config"
export ZZ_TRAY=0
S=/tmp/zzr$$.sock
L=zzprobe-rich-$$
run() {
  local side="$1"; shift
  if [ "$side" = tmux ]; then
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET HOME="$D/h" XDG_CONFIG_HOME="$D/h/config" TMUX_TMPDIR=/tmp "$PIN" -L "$L" "$@"
  else
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET HOME="$D/h" XDG_CONFIG_HOME="$D/h/config" TMUX_TMPDIR=/tmp "$ZZ" --socket "$S" "$@"
  fi
}
trap 'run tmux kill-server >/dev/null 2>&1; run zz kill-server >/dev/null 2>&1; rm -rf "$D" "$S"' EXIT
run tmux -f /dev/null new-session -d -s cap -n win -x 40 -y 8 "ENV= PS1='\$ ' exec /bin/sh" >/dev/null 2>&1
run zz new-session -d -s cap -n win -x 40 -y 8 "ENV= PS1='\$ ' exec /bin/sh" >/dev/null 2>&1
for side in tmux zz; do
  run $side send-keys -t %0 "printf 'plain\n'" Enter >/dev/null
  run $side send-keys -t %0 "printf '\033]8;id=zzid;https://example.com/a\033\\\\LINK\033]8;;\033\\\\\n'" Enter >/dev/null
  run $side send-keys -t %0 "printf '\033[31mRED\033[0m\n'" Enter >/dev/null
done
sleep 2
echo "THE PIN, measured 2026-09-16 on a 40x8 pane holding a plain row, an OSC 8 row and a red row."
echo
for flag in -C -F -H -L -P; do
  printf '===== pin capture-pane -p %s -S 0 -E 6\n' "$flag"
  run tmux capture-pane -p $flag -t %0 -S 0 -E 6 | cat -A
done
echo "===== pin capture-pane -p -R (head, then the line count)"
run tmux capture-pane -p -R -t %0 | head -4 | cat -A
printf 'lines: %s\n' "$(run tmux capture-pane -p -R -t %0 | wc -l)"
echo
echo "ZZ AT THIS LANDING: -C and -L answer, the other four refuse."
for flag in -C -F -H -L -P -R; do
  printf '===== zz capture-pane -p %s -S 0 -E 6\n' "$flag"
  run zz capture-pane -p $flag -t %0 -S 0 -E 6 2>&1 | cat -A | head -10
done
echo
echo "WHY -H CANNOT BE ANSWERED: on the row the pin marks HX, a zz -e capture"
echo "carries no OSC 8 at all, so the retained snapshot did not keep the link."
echo "--- pin -F, the flag column only"
run tmux capture-pane -p -F -t %0 -S 0 -E 6 | cut -c1-3
echo "--- pin -e of that row"
run tmux capture-pane -p -e -t %0 -S 0 -E 6 | sed -n '5p' | cat -A
echo "--- zz -e of that row"
run zz capture-pane -p -e -t %0 -S 0 -E 6 | sed -n '4p' | cat -A
echo
echo "A SECOND MEASUREMENT FOR CLAUSE 2, beyond the one trailing cell it names:"
echo "compare the -e colour spelling on a coloured row."
echo "--- pin"
run tmux capture-pane -p -e -t %0 -S 0 -E 6 | grep RED | cat -A
echo "--- zz"
run zz capture-pane -p -e -t %0 -S 0 -E 6 | grep RED | cat -A
