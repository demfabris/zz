W=/home/demfabris/dev/zz-c11-alias-review6
ZZB=${ZZB:-$W/target/r6-bins/zz_cli-tip}
TMB=$W/compat/.cache/tmux-src/tmux
P=/tmp/claude-1000/-home-demfabris-dev-zz/74f3d4ea-89fe-441e-858d-4b959ebb9bb7/scratchpad/r6/p/${PTAG:-a}
ZS=/tmp/zzr6${PTAG:-a}.sock
TL=zzr6${PTAG:-a}
mkdir -p $P/zzhome $P/tmhome $P/zzlog
unset GITHUB_PERSONAL_ACCESS_TOKEN GH_TOKEN GITHUB_TOKEN CLAUDE_CODE_MESSAGING_TOKEN CLAUDE_CODE_SESSION_ID CLAUDE_CODE_CHILD_SESSION ANTHROPIC_API_KEY OPENAI_API_KEY
zzc() { env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE HOME=$P/zzhome XDG_CONFIG_HOME=$P/zzhome/config ZZ_LOG_DIR=$P/zzlog "$ZZB" --socket $ZS "$@"; }
tmc() { env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp HOME=$P/tmhome XDG_CONFIG_HOME=$P/tmhome/config "$TMB" -L $TL "$@"; }
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
sample() { local s=$1 pane=$2; sidec $s capture-pane -e -p -t "$pane" -S 0 -E ${3:-3} | cat -v; sidec $s display -p -t "$pane" 'cursor=#{cursor_x}:#{cursor_y} hist=#{history_size} size=#{pane_width}x#{pane_height} dead=#{pane_dead} pid=#{?pane_pid,yes,no}'; }
ZZA=(env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE HOME=$P/zzhome XDG_CONFIG_HOME=$P/zzhome/config ZZ_LOG_DIR=$P/zzlog "$ZZB" --socket $ZS)
TMA=(env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp HOME=$P/tmhome XDG_CONFIG_HOME=$P/tmhome/config "$TMB" -L $TL)
sidea() { if [ "$1" = zz ]; then A=("${ZZA[@]}"); else A=("${TMA[@]}"); fi; }
