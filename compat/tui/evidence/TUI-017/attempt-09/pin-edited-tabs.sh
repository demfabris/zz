#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
EVIDENCE="$ROOT/compat/tui/evidence/TUI-017/attempt-09"
PIN="$ROOT/compat/.cache/tmux-src/tmux"
SOCKET="/tmp/zzcr-pin-edits-$$"
trap '"$PIN" -S "$SOCKET" kill-server >/dev/null 2>&1 || true' EXIT
mkdir -p "$EVIDENCE/pin-edits"
while IFS=$'\t' read -r name payload; do
  printf -v command 'printf %%b %q; exec sleep 600' "$payload"
  "$PIN" -S "$SOCKET" -f /dev/null new-session -d -s probe -x 80 -y 24 "$command"
  for ((attempt=0; attempt<100; attempt++)); do
    "$PIN" -S "$SOCKET" capture-pane -p -C -t probe -S 0 -E 4 > "$EVIDENCE/pin-edits/$name.bin"
    if grep -q NEXT "$EVIDENCE/pin-edits/$name.bin"; then break; fi
    sleep 0.02
  done
  grep -q NEXT "$EVIDENCE/pin-edits/$name.bin"
  xxd "$EVIDENCE/pin-edits/$name.bin" > "$EVIDENCE/pin-edits/$name.hex"
  "$PIN" -S "$SOCKET" kill-session -t probe
  printf 'pin %s: captured\n' "$name"
done < "$EVIDENCE/edited-tab-cases.tsv"
