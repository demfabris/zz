#!/usr/bin/env bash
# Pin-only probe: outer pinned tmux drives an inner pinned tmux attached at 80x24.
# usage: pinprobe.sh <command...>   -- runs the command on the inner server, then
# captures the attached screen before, after, and after ending the mode.
set -eEuo pipefail
T="${TMUX_BIN:-/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux}"
SCRATCH="$(mktemp -d /tmp/zzpp.XXXXXX)"
TOKEN="${SCRATCH##*.}"
OUT="zzppo-$TOKEN"
INN="zzppi-$TOKEN"
OH="$SCRATCH/oh"; IH="$SCRATCH/ih"
mkdir -p "$OH/config" "$IH/config"
cleanup() {
  local s=$?
  trap - EXIT
  env HOME="$OH" XDG_CONFIG_HOME="$OH/config" TMUX_TMPDIR=/tmp "$T" -L "$OUT" kill-server >/dev/null 2>&1 || true
  env HOME="$IH" XDG_CONFIG_HOME="$IH/config" TMUX_TMPDIR=/tmp "$T" -L "$INN" kill-server >/dev/null 2>&1 || true
  rm -rf -- "$SCRATCH"
  exit $s
}
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
i set-option -g status-right ''
i set-option -g status-left L
i set-option -g automatic-rename off
i new-window -d -t '=cli:1' -n two "$SH"
for p in $(i list-panes -a -F '#{pane_id}'); do
  i select-pane -t "$p" -T ptitle
  i send-keys -t "$p" "printf 'SCENE-%s\\n' $p" Enter
done
sleep 1
o -f /dev/null new-session -d -s driver -n tmux -x 80 -y 24 "$SCRATCH/attach.sh"
o set-option -g status off
sleep 1.2
cap() { o capture-pane -p -e -S 0 -E 23 -t '=driver:tmux'; }
capp() { o capture-pane -p -S 0 -E 23 -t '=driver:tmux'; }
PANE="$(i list-panes -t '=cli' -F '#{pane_active} #{pane_id}' | awk '$1==1{print $2}')"
CLIENT="$(i list-clients -F '#{client_name}' | head -n1)"
echo "### pane=$PANE client=$CLIENT"
echo "### BEFORE (plain)"; capp | cat -A
ARGS=()
for a in "$@"; do case "$a" in PANE) ARGS+=("$PANE");; CLIENT) ARGS+=("$CLIENT");; *) ARGS+=("$a");; esac; done
set +e
i "${ARGS[@]}" >"$SCRATCH/out" 2>"$SCRATCH/err"; RC=$?
set -e
echo "### rc=$RC"
echo "### stdout"; cat -A "$SCRATCH/out"
echo "### stderr"; cat -A "$SCRATCH/err"
sleep 1.5
echo "### pane_in_mode: $(i display-message -p -t "$PANE" '#{pane_in_mode} #{pane_mode}')"
echo "### AFTER (plain)"; capp | cat -A
echo "### AFTER (styled)"; cap | sed -n '1,30p'
echo "### cursor: $(o display-message -p -t '=driver:tmux' '#{cursor_x},#{cursor_y} flag=#{cursor_flag} shape=#{cursor_shape}')"
