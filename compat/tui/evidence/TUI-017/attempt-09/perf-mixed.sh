#!/usr/bin/env bash
set -euo pipefail
PERF_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
PERF_BINARY="${1:?built binary required}"
PERF_OUTPUT="${2:?fresh output directory required}"
PERF_TMUX="${3:-$PERF_ROOT/compat/.cache/tmux-src/tmux}"
[ ! -e "$PERF_OUTPUT" ] || { printf 'output already exists: %s\n' "$PERF_OUTPUT" >&2; exit 2; }
mkdir -p "$PERF_OUTPUT"
PERF_OUTPUT="$(cd -- "$PERF_OUTPUT" && pwd)"
set -- --self-check "$PERF_BINARY" "$PERF_TMUX"
source <(sed -e "s|^COMPAT_DIR=.*|COMPAT_DIR='$PERF_ROOT/compat'|" -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then$/,$d' "$PERF_ROOT/compat/tui-client-commands.sh")
attach_both_at 80 24
zz_command select-window -t '=cli:win'
python3 - "$SCRATCH_DIR/mixed.bin" <<'PY'
import sys
from pathlib import Path
row = b'ABC\t\x1b[31mnamed\x1b[0m\t\x1b[38;5;1mindexed\x1b[0m\t\x1b[48;5;4mBG\x1b[0m\t' + '\u754c'.encode() + b'\tTAIL\r\n'
Path(sys.argv[1]).write_bytes(row * ((8 * 1024 * 1024) // len(row)))
PY
sha256sum "$ZZ_BIN" "$TMUX_BIN" "$SCRATCH_DIR/mixed.bin" >"$PERF_OUTPUT/sha256.txt"
wc -c <"$SCRATCH_DIR/mixed.bin" >"$PERF_OUTPUT/payload-bytes.txt"
printf 'fixed mixed SGR/tab workload; Bash builtin time (/usr/bin/time unavailable); /bin/cat producer wall time under PTY backpressure; five runs; attached at 80x24; compare identical pane grids and build profiles; marker confirms consumer completion after timing\n' >"$PERF_OUTPUT/method.txt"
perf_marker_ready() {
  capture_plain zz | grep -Fxq "PERF-DONE-$PERF_RUN"
}
for PERF_RUN in 1 2 3 4 5; do
  PERF_INNER="$SCRATCH_DIR/perf-$PERF_RUN.sh"
  {
    printf '#!/usr/bin/env bash\nset -euo pipefail\n'
    printf 'for grid_attempt in {1..50}; do\n  [ "$(stty size)" = "23 80" ] && break\n  sleep 0.05\ndone\n[ "$(stty size)" = "23 80" ]\n'
    printf 'stty size >%q\nsleep 0.05\nstty size >%q\n' "$PERF_OUTPUT/grid-$PERF_RUN-a.txt" "$PERF_OUTPUT/grid-$PERF_RUN-b.txt"
    printf 'cmp %q %q\n' "$PERF_OUTPUT/grid-$PERF_RUN-a.txt" "$PERF_OUTPUT/grid-$PERF_RUN-b.txt"
    printf 'TIMEFORMAT=%q\n{ time /bin/cat %q; } 2>%q\n' $'real %R\nuser %U\nsys %S' "$SCRATCH_DIR/mixed.bin" "$PERF_OUTPUT/time-$PERF_RUN.txt"
    printf "printf '\\033[0m\\r\\nPERF-DONE-%s\\r\\n'\nexec sleep 600\n" "$PERF_RUN"
  } >"$PERF_INNER"
  printf -v PERF_COMMAND 'bash %q' "$PERF_INNER"
  zz_command respawn-pane -k -t '=cli:win' "$PERF_COMMAND"
  wait_for "mixed cat marker run $PERF_RUN" perf_marker_ready
  capture_plain zz >"$PERF_OUTPUT/screen-$PERF_RUN.txt"
  printf 'run=%s marker=ready grid=%s\n' "$PERF_RUN" "$(cat "$PERF_OUTPUT/grid-$PERF_RUN-b.txt")" | tee -a "$PERF_OUTPUT/readiness.txt"
  cat "$PERF_OUTPUT/time-$PERF_RUN.txt"
done
printf 'performance probe complete: runs=5 markers=5\n'
