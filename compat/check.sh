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
  compat_manifest_tests::args_parse_gaps_match_the_pinned_oracle \
  compat_manifest_tests::daemon_invalid_flag_runtime_inventory_matches_the_pin \
  compat_manifest_tests::command_flag_fixture_matches_the_pin \
  compat_manifest_tests::positional_maximum_runtime_inventory_matches_the_pin \
  compat_manifest_tests::positional_minimum_runtime_inventory_matches_the_pin \
  compat_manifest_tests::scoped_format_contexts_and_modifiers_match_the_pinned_oracle \
  compat_manifest_tests::option_format_hook_and_default_key_items_match_pinned_inventories; do
  grep -Fqx -- "$required_test: test" <<<"$test_list" || {
    printf 'error: required compatibility manifest test is missing: %s\n' "$required_test" >&2
    exit 1
  }
done
cargo test -p zz-mux --lib

daemon_test_list="$(cargo test -p zz-daemon --lib -- --list)"
for daemon_test in \
  daemon::tests::daemon_context_format_registration_matches_the_oracle \
  daemon::tests::pinned_hook_producer_partition_matches_the_oracle \
  status::tests::daemon_delegated_format_consumers_match_mux_inventory; do
  grep -Fqx -- "$daemon_test: test" <<<"$daemon_test_list" || {
    printf 'error: required compatibility manifest test is missing: %s\n' "$daemon_test" >&2
    exit 1
  }
  cargo test -p zz-daemon --lib "$daemon_test" -- --exact
done
