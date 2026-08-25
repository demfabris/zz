#!/usr/bin/env bash
set -euo pipefail

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"

tmux_bin="$("$COMPAT_DIR/fetch-tmux.sh")"
python3 "$COMPAT_DIR/tmux-oracle.py" --check --tmux "$tmux_bin"
python3 "$COMPAT_DIR/tmux-tracker.py" check
cd "$REPO_DIR"
test_list="$(cargo test -p zz-mux --lib -- --list)"
for required_test in \
  compat_manifest_tests::command_and_flag_gaps_match_the_pinned_oracle \
  compat_manifest_tests::option_format_hook_and_default_key_items_match_pinned_inventories; do
  grep -Fqx -- "$required_test: test" <<<"$test_list" || {
    printf 'error: required compatibility manifest test is missing: %s\n' "$required_test" >&2
    exit 1
  }
done
cargo test -p zz-mux --lib
