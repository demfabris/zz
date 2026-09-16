set -u
PIN="${ZZ_COMPAT_TMUX:-$PWD/compat/.cache/tmux-src/tmux}"
ZZ="${ZZ_BIN:-$PWD/target/debug/zz}"
D=/tmp/zzlockev.$$; rm -rf "$D"; mkdir -p "$D/h/config"
export ZZ_TRAY=0
S=/tmp/zzlkev$$.sock
L=zzprobe-ev-$$
run() {
  local side="$1"; shift
  if [ "$side" = tmux ]; then
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET HOME="$D/h" XDG_CONFIG_HOME="$D/h/config" TMUX_TMPDIR=/tmp "$PIN" -L "$L" "$@"
  else
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET HOME="$D/h" XDG_CONFIG_HOME="$D/h/config" TMUX_TMPDIR=/tmp "$ZZ" --socket "$S" "$@"
  fi
}
trap 'run tmux kill-server >/dev/null 2>&1; run zz kill-server >/dev/null 2>&1; rm -rf "$D" "$S"' EXIT
echo "clientless throwaway servers, one session cli:win, lock-command true on both"
run tmux -f /dev/null new-session -d -s cli -n win -x 80 -y 24 "ENV= PS1='\$ ' exec /bin/sh" >/dev/null 2>&1
run zz new-session -d -s cli -n win -x 80 -y 24 "ENV= PS1='\$ ' exec /bin/sh" >/dev/null 2>&1
run tmux set-option -g lock-command true >/dev/null; run zz set-option -g lock-command true >/dev/null
probe() {
  local label="$1"; shift
  local out err rc
  err="$D/err"
  for side in tmux zz; do
    out=$(run $side "$@" 2>"$err"); rc=$?
    printf '  %-5s rc=%s stdout=[%s] stderr=[%s]\n' "$side" "$rc" "$out" "$(cat "$err")"
  done
  printf -- '--- %s\n' "$label"
}
probe2() {
  printf -- '--- %s\n' "$*"
  local out err rc
  err="$D/err"
  for side in tmux zz; do
    out=$(run $side "$@" 2>"$err"); rc=$?
    printf '  %-5s rc=%s stdout=[%s] stderr=[%s]\n' "$side" "$rc" "$out" "$(cat "$err" | tr '\n' '|')"
  done
}
echo
echo "== A. arity, flags and target validation of the three lock commands"
probe2 lock-server zzcc-extra
probe2 lock-server -t zzcc-nope
probe2 lock-session zzcc-extra
probe2 lock-session -t zzcc-nope
probe2 lock-session
probe2 lock-client zzcc-extra
probe2 lock-client -t
probe2 lock-client -t /dev/zzcc-nope
probe2 lock-client
echo
echo "== B. the one hook name: cmd-lock-server.c gives all three CMD_AFTERHOOK, so"
echo "      the pin fires after-lock-<command name> and only after-lock-server exists"
probe2 set-hook -g after-lock-server 'set -g @m yes'
probe2 set-hook -g after-lock-session 'set -g @m yes'
probe2 set-hook -g after-lock-client 'set -g @m yes'
probe2 show-options -gqv @m
probe2 lock-session -t =cli
probe2 show-options -gqv @m
probe2 lock-client
probe2 show-options -gqv @m
probe2 lock-server
probe2 show-options -gqv @m
echo
echo "== C. both knobs, stored at both scopes and validated, arming nothing"
probe2 show-options -g lock-after-time
probe2 show-options -g lock-command
probe2 set-option -g lock-after-time zzcc-nope
probe2 set-option -t cli lock-command zzcc-locker
probe2 show-options -t cli lock-command
probe2 set-option -t cli lock-after-time 45
probe2 show-options -t cli lock-after-time
probe2 set-option -t cli -u lock-command
probe2 set-option -t cli -u lock-after-time
probe2 show-options -t cli lock-command
echo
echo "== D. THE FINDING, and the control that shows it is not a lock fact:"
echo "      a session target carrying a window or pane suffix"
probe2 lock-session -t =cli:win
probe2 lock-session -t =nosuch:win
probe2 lock-session -t =cli:nosuchwin
probe2 lock-session -t %0
probe2 lock-session -t %99
probe2 lock-session -t =cli:win.9
echo "  CONTROL, commands with no lock in them at all:"
probe2 has-session -t =cli:win
probe2 list-windows -t =cli:win
