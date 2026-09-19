W=/home/demfabris/dev/zz-c11-alias-fix
ZZB=${ZZB:-$W/target/debug/zz_cli}
TMB=$W/compat/.cache/tmux-src/tmux
P=/tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/probe
ZS=/tmp/zzp6.sock
mkdir -p $P/zzhome $P/tmhome
zzc() { env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE HOME=$P/zzhome XDG_CONFIG_HOME=$P/zzhome/config ZZ_LOG_DIR=$P/zzlog "$ZZB" --socket $ZS "$@"; }
tmc() { env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp HOME=$P/tmhome XDG_CONFIG_HOME=$P/tmhome/config "$TMB" -L zzp6 "$@"; }
sidec() { local s=$1; shift; if [ "$s" = zz ]; then zzc "$@"; else tmc "$@"; fi; }
start_both() {
  (setsid zzc -f /dev/null daemon >$P/daemon.out 2>&1 &)
  for i in $(seq 100); do [ -S $ZS ] && break; sleep 0.05; done
  for s in zz tmux; do
    sidec $s -f /dev/null new-session -d -s cs -n win -x 80 -y 24 "ENV= PS1='\$ ' exec /bin/sh"
    sidec $s set -g status off
  done
}
stop_both() { zzc kill-server >/dev/null 2>&1; tmc kill-server >/dev/null 2>&1; rm -f $ZS; }
