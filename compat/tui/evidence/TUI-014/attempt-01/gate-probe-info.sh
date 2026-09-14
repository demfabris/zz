#!/usr/bin/env bash
set -u
TMUX_BIN=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux
ZZ_BIN=/home/demfabris/dev/zz-gate-target/debug/zz
D=$(mktemp -d /tmp/zzprobe.XXXXXX)
TOK=${D##*.}
OUT=zzprobe-o$TOK
INN=zzprobe-i$TOK
SOCK=/tmp/zzprobe-$TOK.sock
ZH=$D/zzhome; TH=$D/tmuxhome; OH=$D/outerhome
mkdir -p "$ZH" "$TH" "$OH"
scrub(){ env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME TMUX_TMPDIR=/tmp "$@"; }
o(){ scrub HOME="$OH" XDG_CONFIG_HOME="$OH/config" "$TMUX_BIN" -L "$OUT" "$@"; }
z(){ scrub HOME="$ZH" XDG_CONFIG_HOME="$ZH/config" ZZ_LOG_DIR="$D/zzlog" "$ZZ_BIN" --socket "$SOCK" "$@"; }
t(){ scrub HOME="$TH" XDG_CONFIG_HOME="$TH/config" "$TMUX_BIN" -L "$INN" -f /dev/null "$@"; }
cleanup(){ o kill-server >/dev/null 2>&1; z kill-server >/dev/null 2>&1; t kill-server >/dev/null 2>&1; rm -rf -- "$D"; rm -f "$SOCK"; }
trap cleanup EXIT
waitfor(){ local l="$1"; shift; for ((i=0;i<200;i++)); do "$@" >/dev/null 2>&1 && return 0; sleep 0.05; done; echo "TIMEOUT: $l" >&2; return 1; }

# inner servers
t new-session -d -s cho -n win "ENV= PS1='\$ ' exec /bin/sh" || exit 2
z new-session -d -s cho -n win "ENV= PS1='\$ ' exec /bin/sh" || exit 2
for s in z t; do
  $s set-option -g status-right '' ; $s set-option -g status-left 'L'
  $s set-option -g automatic-rename off
done
z select-pane -T ptitle; t select-pane -T ptitle

cat > "$D/az.sh" <<EOF
#!/usr/bin/env bash
exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u XDG_STATE_HOME HOME=$ZH XDG_CONFIG_HOME=$ZH/config ZZ_LOG_DIR=$D/zzlog TMUX_TMPDIR=/tmp $ZZ_BIN --socket $SOCK attach-session -t =cho
EOF
cat > "$D/at.sh" <<EOF
#!/usr/bin/env bash
exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u XDG_STATE_HOME HOME=$TH XDG_CONFIG_HOME=$TH/config TMUX_TMPDIR=/tmp $TMUX_BIN -L $INN -f /dev/null attach-session -t =cho
EOF
chmod +x "$D/az.sh" "$D/at.sh"

o -f /dev/null new-session -d -s driver -x 80 -y 24 -n zz "$D/az.sh" || exit 2
o new-window -d -t =driver: -n tmux "$D/at.sh" || exit 2
o set-option -g status off
waitfor "zz client" bash -c "z list-clients -F '#{client_session}' | grep -q cho"
waitfor "tmux client" bash -c "t list-clients -F '#{client_session}' | grep -q cho"
sleep 1
send(){ o send-keys -t "=driver:$1" "$2"; }
cap(){ o capture-pane -p -S 0 -E 23 -t "=driver:$1"; }

echo "=== STEP 1: prefix D (client mode) ==="
for w in zz tmux; do send $w C-b; sleep 0.3; send $w D; done
sleep 2
echo "--- zz ---"; cap zz | cat -A | sed 's/\$$//' | head -24
echo "--- tmux ---"; cap tmux | cat -A | sed 's/\$$//' | head -24

echo "=== STEP 2: i (info view) ==="
for w in zz tmux; do send $w i; done
sleep 2
echo "--- zz info ---"; cap zz | head -24
echo "--- tmux info ---"; cap tmux | head -24

echo "=== STEP 3: i again then Enter (default command) ==="
for w in zz tmux; do send $w i; done
sleep 1
echo "clients before: zz=$(z list-clients -F '#{client_name}' | tr '\n' ' ') tmux=$(t list-clients -F '#{client_name}' | tr '\n' ' ')"
for w in zz tmux; do send $w Enter; done
sleep 2
echo "clients after Enter: zz=$(z list-clients -F '#{client_name}' | tr '\n' ' ') tmux=$(t list-clients -F '#{client_name}' | tr '\n' ' ')"
echo "--- zz after Enter ---"; cap zz | head -6
echo "--- tmux after Enter ---"; cap tmux | head -6
