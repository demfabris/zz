#!/usr/bin/env bash
set -uo pipefail

SELF="$(readlink -f "$0")"
CARGO="${ZZ_CARGO_BIN:-}"
if [ -z "$CARGO" ]; then
  for CANDIDATE in $(type -ap cargo); do
    [ "$(readlink -f "$CANDIDATE")" = "$SELF" ] && continue
    CARGO="$CANDIDATE"
    break
  done
fi
if [ -n "${ZZ_CARGO_SLOT_HELD:-}" ]; then
  exec "$CARGO" "$@"
fi
export ZZ_CARGO_SLOT_HELD=1

if [ "$(uname -s)" = Darwin ]; then
  MEM_GB=$(( $(sysctl -n hw.memsize) / 1073741824 ))
else
  MEM_GB=$(( $(awk '/MemTotal/ {print $2}' /proc/meminfo) / 1048576 ))
fi
if [ "$MEM_GB" -le 20 ]; then
  DEF_JOBS=2 DEF_CAP=4G
elif [ "$MEM_GB" -le 40 ]; then
  DEF_JOBS=3 DEF_CAP=8G
else
  DEF_JOBS=4 DEF_CAP=12G
fi
JOBS="${ZZ_CARGO_JOBS:-$DEF_JOBS}"
CAP="${ZZ_CARGO_CAP:-$DEF_CAP}"
SLOTS="${ZZ_CARGO_SLOTS:-2}"
LOCKDIR="${XDG_RUNTIME_DIR:-/tmp}"

ARGS=()
case "${1:-}" in
  fmt|metadata|tree) ARGS=("$@") ;;
  *)
    for ARG in "$@"; do
      if [ "$ARG" = -- ] && [ -n "$JOBS" ]; then
        ARGS+=(--jobs "$JOBS")
        JOBS=""
      fi
      ARGS+=("$ARG")
    done
    [ -z "$JOBS" ] || ARGS+=(--jobs "$JOBS")
    ;;
esac

RUN=("$CARGO" "${ARGS[@]}")
if command -v systemd-run >/dev/null 2>&1; then
  RUN=(systemd-run --user --scope -q -p MemoryMax="$CAP" -p MemorySwapMax=2G "${RUN[@]}")
fi
if [ "$(uname -s)" != Linux ] || ! command -v flock >/dev/null 2>&1; then
  exec "${RUN[@]}"
fi

for ((i = 0; i < SLOTS; i++)); do
  flock -n -E 75 -o "$LOCKDIR/zz-cargo-slot-$i.lock" "${RUN[@]}"
  rc=$?
  [ "$rc" -eq 75 ] || exit "$rc"
done
exec flock -w 3600 -o "$LOCKDIR/zz-cargo-slot-$((RANDOM % SLOTS)).lock" "${RUN[@]}"
