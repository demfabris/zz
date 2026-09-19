#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
E="$ROOT/compat/tui/evidence/TUI-017/attempt-10"
B="$ROOT/target/residuals-2/perf"
CPU="${1:-14}"
OUT="$E/native-samples.jsonl"
: > "$OUT"
run() {
  local binary="$1" workload="$2" variant="$3" block="$4" pair="$5"
  printf '{"binary":"%s","block":%s,"pair":%s,"sample":%s}\n' "$binary" "$block" "$pair" \
    "$(taskset -c "$CPU" setarch x86_64 -R "$B/$binary" "$workload" "$variant")" >> "$OUT"
}
for _ in $(seq 1 20); do
  taskset -c "$CPU" "$B/candidate" mixed tabs >/dev/null
  taskset -c "$CPU" "$B/main" mixed tabs >/dev/null
done
for block in 1 2 3; do
  for workload in mixed alltabs htsevery widestops overwrite edits reflow; do
    for variant in tabs spaces; do
      for pair in 1 2 3 4 5; do
        if [ $((pair % 2)) -eq 1 ]; then
          run candidate "$workload" "$variant" "$block" "$pair"
          run main "$workload" "$variant" "$block" "$pair"
        else
          run main "$workload" "$variant" "$block" "$pair"
          run candidate "$workload" "$variant" "$block" "$pair"
        fi
      done
    done
  done
done
printf 'native benchmark: 3 blocks x 7 workloads x 2 variants x 5 ABBA pairs, cpu %s, ASLR off per child, 20 warm-up pairs, exit=0\n' "$CPU"
