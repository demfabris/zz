#!/usr/bin/env bash
# Renders bench/results/results.jsonl as a markdown report.
#
#   bench/summarize.sh                 # default results file
#   bench/summarize.sh path/to.jsonl   # any JSONL produced by inner.sh
#
# The last record wins for a given (label, test) pair, so re-running one
# terminal updates the table without needing --fresh.
set -euo pipefail

BENCH_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
JSONL="${1:-$BENCH_DIR/results/results.jsonl}"

command -v jq >/dev/null 2>&1 || {
	echo "summarize.sh: jq is required" >&2
	exit 1
}
[ -s "$JSONL" ] || {
	echo "summarize.sh: no results at $JSONL (run bench/run.sh)" >&2
	exit 1
}

jq -s -r '
  def latest: group_by(.label + "\u0000" + .test) | map(sort_by(.timestamp) | last);
  def num(n): (n * 100 | round) / 100;
  def grid: "\(.cols)x\(.lines)";

  def cat_table($rows; $test):
    ($rows | map(select(.test == $test))
           | sort_by(if .status == "ok" then -(.mb_per_s) else 1e18 end)) as $r
    | if ($r | length) == 0 then ["_no results_"]
      else
        ["| terminal | median | MB/s | all runs (ms) | grid | timer |",
         "| --- | ---: | ---: | --- | ---: | --- |"]
        + ($r | map(
            if .status == "ok" then
              "| `\(.label)` | \(num(.median_ms)) ms | **\(.mb_per_s)** | \(.times_ms | map(num(.) | tostring) | join(", ")) | \(grid) | \(.tool) |"
            else
              "| `\(.label)` | – | – | _skipped: \(.reason)_ | \(grid) | – |"
            end))
      end;

  def doom_table($rows):
    ($rows | map(select(.test == "doom-fire"))
           | sort_by(if .status == "ok" then -(.fps) else 1e18 end)) as $r
    | if ($r | length) == 0 then ["_no results_"]
      else
        ["| terminal | fps | duration | grid | capture |",
         "| --- | ---: | ---: | ---: | --- |"]
        + ($r | map(
            if .status == "ok" then
              "| `\(.label)` | **\(num(.fps))** | \(.seconds)s | \(grid) | \(.capture) |"
            else
              "| `\(.label)` | – | – | \(grid) | _skipped: \(.reason)_ |"
            end))
      end;

  def headline($rows; $test; $unit; $field):
    ($rows | map(select(.test == $test and .status == "ok"))) as $r
    | ($r | map(select(.label == "zz")) | first) as $zz
    | ($r | map(select(.label == "ghostty+tmux")) | first) as $gt
    | if $zz == null or $gt == null then empty
      else
        "- **\($test)**: zz \(num($zz[$field])) \($unit) vs ghostty+tmux \(num($gt[$field])) \($unit) (**\(num($zz[$field] / $gt[$field]))x**)"
      end;

  latest as $rows
  | ($rows | map(select(.status == "ok")) | map(grid) | unique) as $grids
  | ($rows | map(select(.fixture_sha12 != null) | {test, sha: .fixture_sha12})
           | group_by(.test) | map({test: .[0].test, shas: (map(.sha) | unique)})
           | map(select((.shas | length) > 1))) as $sha_mismatch
  | ($rows | map(select(.status == "ok" and .tool != null) | .tool) | unique) as $tools
  | ($rows | map(.hw_model) | unique | join(", ")) as $hw
  | ($rows | map(.macos) | unique | join(", ")) as $os
  | ($rows | map(.timestamp) | max) as $when
  | [
      "# Terminal IO throughput",
      "",
      "\($hw) · macOS \($os) · \($when)",
      "",
      "## Headline",
      ""
    ]
    + ([headline($rows; "cat-ascii"; "MB/s"; "mb_per_s"),
        headline($rows; "cat-unicode"; "MB/s"; "mb_per_s"),
        headline($rows; "doom-fire"; "fps"; "fps")] | if length == 0 then ["_zz and ghostty+tmux have not both reported yet._"] else . end)
    + [
      "",
      "## cat 150 MiB ASCII",
      ""
    ] + cat_table($rows; "cat-ascii")
    + [
      "",
      "## cat 150 MiB mixed UTF-8",
      ""
    ] + cat_table($rows; "cat-unicode")
    + [
      "",
      "## DOOM-fire-zig",
      ""
    ] + doom_table($rows)
    + [
      "",
      "## Fairness checks",
      ""
    ]
    + (if ($grids | length) > 1 then
         ["- ⚠ **grid mismatch**: terminals reported \($grids | join(", ")). Resize them to match and re-run; throughput scales with cell count. (A one-row gap on a tmux label is its status line, which we leave on deliberately.)"]
       else
         ["- grid: \($grids | join(", ")) (consistent)"]
       end)
    + (if ($sha_mismatch | length) > 0 then
         ($sha_mismatch | map("- ⚠ **fixture mismatch** for \(.test): saw \(.shas | join(", ")). Regenerate with `bench/gen-fixtures.sh --force` and re-run everything."))
       else
         ["- fixtures: identical sha256 across terminals"]
       end)
    + (if ($tools | length) > 1 then
         ["- ⚠ **timer mismatch**: \($tools | join(", ")). Install hyperfine so every terminal uses the same one."]
       else
         ["- timer: \($tools | join(", "))"]
       end)
    + [
      "",
      "_Throughput here is partly a measure of how aggressively a terminal batches frames while draining its pty; see the caveats in bench/README.md._"
    ]
  | .[]
' "$JSONL"
