#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
RESULTS="$ROOT/compat/tui/evidence/TUI-017/attempt-09/final/native-final-v4"
BINARIES="$ROOT/target/capture-residuals-perf"
mkdir "$RESULTS"
for run in {1..100}; do
  taskset -c 6 "$BINARIES/native-fixed-v4" >/dev/null
  taskset -c 6 "$BINARIES/native-main" >/dev/null
done
for workload in tabs spaces; do
  suffix=''
  [ "$workload" != spaces ] || suffix='-spaces'
  for run in 1 2 3 4 5; do
    taskset -c 6 "$BINARIES/native-fixed-v4$suffix" > "$RESULTS/fixed-$workload-$run.json"
    taskset -c 6 "$BINARIES/native-main$suffix" > "$RESULTS/main-$workload-$run.json"
  done
done
printf 'native benchmark: five alternating pairs per workload, CPU 6, 100 warm-up pairs, exit=0\n'
