#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
B="$ROOT/target/residuals-2/perf"
for workload in mixed alltabs htsevery widestops overwrite edits reflow; do
  for variant in tabs spaces; do
    for binary in main indexed-only candidate; do
      for run in 1 2 3; do
        printf '{"binary":"%s","run":%s,"sample":%s}\n' "$binary" "$run" "$(taskset -c 8 "$B/$binary" "$workload" "$variant")"
      done
    done
  done
done
