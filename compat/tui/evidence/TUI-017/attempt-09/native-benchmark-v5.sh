#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
RESULTS="$ROOT/compat/tui/evidence/TUI-017/attempt-09/final/native-final-v5"
BINARIES="$ROOT/target/capture-residuals-perf"
mkdir "$RESULTS"
for run in {1..100}; do
  taskset -c 12 setarch x86_64 -R "$BINARIES/native-fixed-v5" >/dev/null
  taskset -c 12 setarch x86_64 -R "$BINARIES/native-main" >/dev/null
done
for block in 1 2 3; do
  workloads=(tabs spaces)
  [ "$block" -ne 2 ] || workloads=(spaces tabs)
  for workload in "${workloads[@]}"; do
  suffix=''
  [ "$workload" != spaces ] || suffix='-spaces'
  for run in 1 2 3 4 5; do
    taskset -c 12 setarch x86_64 -R "$BINARIES/native-fixed-v5$suffix" > "$RESULTS/fixed-$workload-$block-$run.json"
    taskset -c 12 setarch x86_64 -R "$BINARIES/native-main$suffix" > "$RESULTS/main-$workload-$block-$run.json"
  done
done
done
printf 'native benchmark: three blocks of five alternating pairs per workload, CPU 12, ASLR disabled for each benchmark child, 100 warm-up pairs, exit=0\n'
