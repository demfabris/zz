#!/usr/bin/env bash
# Unpatch perf: pristine vs patched libghostty-vt on edit-heavy workloads.
# Fixed commands only: two zig builds, two cc links, fixed workload binaries.
set -u
REPO=/home/demfabris/dev/zz-c11-capture-residuals
P=$REPO/target/unpatch-perf
CPU=15
LOG=$P/perf-build.log
: > "$LOG"

build_one() {
  local src="$1" prefix="$2" cache="$3"
  cd "$src" || exit 2
  zig build -Demit-lib-vt=true -Doptimize=ReleaseSafe -Dcpu=baseline \
    -Demit-xcframework=false -Dapp-runtime=none \
    --prefix "$prefix" --cache-dir "$cache" >>"$LOG" 2>&1
}

echo "building pristine lib..."
build_one "$P/ghostty-pristine" "$P/pristine-install" "$P/zig-cache-pristine"
echo "building patched lib..."
build_one "$P/ghostty-patched" "$P/patched-install" "$P/zig-cache-patched"
echo "both libs built"

HARNESS=$REPO/compat/tui/evidence/TUI-017/attempt-10/native-perf-review.c
cc -O2 -Wall -Wextra -Werror "$HARNESS" -I "$P/pristine-install/include" \
  "$P/pristine-install/lib/libghostty-vt.a" -pthread -lm -ldl -o "$P/perf-pristine" >>"$LOG" 2>&1
cc -O2 -Wall -Wextra -Werror "$HARNESS" -I "$P/patched-install/include" \
  "$P/patched-install/lib/libghostty-vt.a" -pthread -lm -ldl -o "$P/perf-patched" >>"$LOG" 2>&1
echo "both drivers linked"

OUT=$P/samples.jsonl
: > "$OUT"
for _ in $(seq 1 5); do
  taskset -c "$CPU" "$P/perf-pristine" edits tabs >/dev/null
  taskset -c "$CPU" "$P/perf-patched" edits tabs >/dev/null
done
echo "warmup done"
for workload in edits overwrite; do
  for variant in tabs spaces; do
    for pair in $(seq 1 15); do
      if [ $((pair % 2)) -eq 1 ]; then
        FIRST=pristine; SECOND=patched
      else
        FIRST=patched; SECOND=pristine
      fi
      printf '{"binary":"%s","pair":%s,%s}\n' "$FIRST" "$pair" \
        "$(taskset -c "$CPU" setarch x86_64 -R "$P/perf-$FIRST" "$workload" "$variant" | tr -d '{}\n')" >>"$OUT"
      printf '{"binary":"%s","pair":%s,%s}\n' "$SECOND" "$pair" \
        "$(taskset -c "$CPU" setarch x86_64 -R "$P/perf-$SECOND" "$workload" "$variant" | tr -d '{}\n')" >>"$OUT"
    done
  done
done
echo "samples done: $(wc -l <"$OUT")"
python3 - "$OUT" <<'EOF'
import json, statistics, sys
rows = [json.loads(line) for line in open(sys.argv[1])]
for workload in ('edits', 'overwrite'):
    for variant in ('tabs', 'spaces'):
        for binary in ('pristine', 'patched'):
            xs = [r['cpu_seconds'] for r in rows if r['workload'] == workload and r['variant'] == variant and r['binary'] == binary]
            print(f"{workload}/{variant}/{binary}: n={len(xs)} median_cpu={statistics.median(xs):.6f} min={min(xs):.6f}")
        xp = [r['cpu_seconds'] for r in rows if r['workload'] == workload and r['variant'] == variant and r['binary'] == 'pristine']
        xt = [r['cpu_seconds'] for r in rows if r['workload'] == workload and r['variant'] == variant and r['binary'] == 'patched']
        print(f"{workload}/{variant}: patched-vs-pristine {(statistics.median(xt) / statistics.median(xp) - 1) * 100:+.2f}%")
EOF
echo PERF-DONE
