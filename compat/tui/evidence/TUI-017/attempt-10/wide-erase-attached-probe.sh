#!/usr/bin/env bash
set -uo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
ZZ="$1"
TMUX_PIN="$ROOT/compat/.cache/tmux-src/tmux"
SCRATCH="$(mktemp -d /tmp/zzwe.XXXXXX)"
trap 'rm -rf "$SCRATCH"' EXIT
mkdir -p "$SCRATCH/config"
ENV=(env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE HOME="$SCRATCH" XDG_CONFIG_HOME="$SCRATCH/config" TMUX_TMPDIR=/tmp)
side_cmd() {
  if [ "$1" = zz ]; then printf '%s\n' "$ZZ" --socket "$SCRATCH/zz.sock"; else printf '%s\n' "$TMUX_PIN" -S "$SCRATCH/pin.sock" -f /dev/null; fi
}
capture_scene() {
  local side="$1" payload="$2" flags="$3"
  local -a cmd outer=("${ENV[@]}" "$TMUX_PIN" -S "$SCRATCH/outer.sock" -f /dev/null)
  mapfile -t cmd < <(side_cmd "$side")
  rm -f "$SCRATCH/go"
  "${ENV[@]}" "${cmd[@]}" new-session -d -s cli -n win -x 100 -y 30 \
    "printf READY; while [ ! -f $SCRATCH/go ]; do sleep .05; done; printf '%b' '$payload'; exec sleep 600"
  until "${ENV[@]}" "${cmd[@]}" capture-pane -p -t =cli: | grep -q READY; do sleep .05; done
  local attach
  attach="$(printf '%q ' "${ENV[@]}" "${cmd[@]}" attach-session -t =cli)"
  "${outer[@]}" new-session -d -s outer -x 100 -y 31 "$attach"
  "${outer[@]}" set-option -g status off >/dev/null
  "${outer[@]}" new-window -d -t =outer "$attach"
  "${ENV[@]}" "${cmd[@]}" resize-window -t =cli: -x 100 -y 30
  until [ "$("${ENV[@]}" "${cmd[@]}" display-message -p -t =cli: '#{pane_width}x#{pane_height}')" = 100x30 ]; do sleep .05; done
  sleep 0.2
  touch "$SCRATCH/go"
  until "${ENV[@]}" "${cmd[@]}" capture-pane -p -t =cli: | grep -q NEXT; do sleep .05; done
  sleep 0.3
  # shellcheck disable=SC2086
  "${ENV[@]}" "${cmd[@]}" capture-pane -p -t =cli: $flags -S 0 -E 4
  "${outer[@]}" kill-server >/dev/null 2>&1
  "${ENV[@]}" "${cmd[@]}" kill-server >/dev/null 2>&1
  sleep 0.2
}
differences=0
for scene in line display clear region; do
  case "$scene" in
  line) payload='\033[H\033[2J\033[41m\033[2K\033[0m\r\nNEXT' ;;
  display) payload='\033[H\033[2J\033[41m\033[2J\033[0m\r\nNEXT' ;;
  clear) payload='\033[H\033[2J\033[41m\033[H\033[2J\033[0m\r\nNEXT' ;;
  region) payload='\033[H\033[2J\033[2;4r\033[2;1H\033[41m\033[2K\033[0m\r\nNEXT\033[r' ;;
  esac
  for mode in escape padding; do
    flags='-C -e'
    [ "$mode" = escape ] || flags='-e -N'
    pin="$(capture_scene tmux "$payload" "$flags" | od -An -c | tr -s ' ')"
    zz="$(capture_scene zz "$payload" "$flags" | od -An -c | tr -s ' ')"
    if [ "$pin" = "$zz" ]; then
      printf 'same  100x30/%s/%s\n' "$scene" "$mode"
    else
      differences=$((differences + 1))
      printf 'DIFF  100x30/%s/%s\n' "$scene" "$mode"
    fi
  done
done
printf 'attached wide erase probe: %s differences\n' "$differences"
[ "$differences" -eq 0 ]
