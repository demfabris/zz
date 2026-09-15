#!/usr/bin/env bash
set -eEuo pipefail
T="${TMUX_BIN:-/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux}"
SCRATCH="$(mktemp -d /tmp/zzsp.XXXXXX)"
TOKEN="${SCRATCH##*.}"
OUT="zzspo-$TOKEN"; INN="zzspi-$TOKEN"
OH="$SCRATCH/oh"; IH="$SCRATCH/ih"; mkdir -p "$OH/config" "$IH/config"
cleanup() { local s=$?; trap - EXIT
  env HOME="$OH" XDG_CONFIG_HOME="$OH/config" TMUX_TMPDIR=/tmp "$T" -L "$OUT" kill-server >/dev/null 2>&1 || true
  env HOME="$IH" XDG_CONFIG_HOME="$IH/config" TMUX_TMPDIR=/tmp "$T" -L "$INN" kill-server >/dev/null 2>&1 || true
  rm -rf -- "$SCRATCH"; exit $s; }
trap cleanup EXIT INT TERM
o() { env -u TMUX -u TMUX_PANE HOME="$OH" XDG_CONFIG_HOME="$OH/config" TMUX_TMPDIR=/tmp "$T" -L "$OUT" "$@"; }
i() { env -u TMUX -u TMUX_PANE HOME="$IH" XDG_CONFIG_HOME="$IH/config" TMUX_TMPDIR=/tmp "$T" -L "$INN" "$@"; }
cat > "$SCRATCH/attach.sh" <<ATT
#!/usr/bin/env bash
exec env -u TMUX -u TMUX_PANE HOME=$IH XDG_CONFIG_HOME=$IH/config TMUX_TMPDIR=/tmp $T -L $INN attach-session -t =cli
ATT
chmod +x "$SCRATCH/attach.sh"
SH="ENV= PS1='\$ ' exec /bin/sh"
i -f /dev/null new-session -d -s cli -n win -x 80 -y 24 "$SH"
i set-option -g status-right ''; i set-option -g status-left L; i set-option -g automatic-rename off
i new-window -d -t '=cli:1' -n two "$SH"
for p in $(i list-panes -a -F '#{pane_id}'); do i select-pane -t "$p" -T ptitle; i send-keys -t "$p" "printf 'SCENE-%s\\n' $p" Enter; done
sleep 1
o -f /dev/null new-session -d -s driver -n tmux -x 80 -y 24 "$SCRATCH/attach.sh"
o set-option -g status off
sleep 1.2
state() { i list-sessions -F 'S #{session_name} #{session_windows} #{session_attached}'; i list-clients -F 'C #{client_session} #{client_width}x#{client_height} #{client_prefix}'; }
echo "### STATE BEFORE"; state
echo "### pane process: $(o display-message -p -t '=driver:tmux' '#{pane_pid} #{pane_current_command}')"
set +e
i suspend-client >"$SCRATCH/out" 2>"$SCRATCH/err"; RC=$?
set -e
echo "### rc=$RC out=[$(cat "$SCRATCH/out")] err=[$(cat "$SCRATCH/err")]"
sleep 2
echo "### STATE AFTER"; state
echo "### pane process after: $(o display-message -p -t '=driver:tmux' '#{pane_pid} #{pane_current_command}')"
echo "### SCREEN AFTER"; o capture-pane -p -S 0 -E 23 -t '=driver:tmux' | cat -A
echo "### cursor: $(o display-message -p -t '=driver:tmux' '#{cursor_x},#{cursor_y} flag=#{cursor_flag} shape=#{cursor_shape}')"
echo "### ps state: $(ps -o stat= -p "$(o display-message -p -t '=driver:tmux' '#{pane_pid}')" 2>/dev/null)"
