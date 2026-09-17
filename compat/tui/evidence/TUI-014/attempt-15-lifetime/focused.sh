#!/usr/bin/env bash
set -eEuo pipefail
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then/,$d' compat/tui-client-commands.sh)"
attach_both_at 80 24
switch_lifetime_cases
switch_lifetime_self_checks
printf 'lifetime checks=%s failures=%s sabotage_failures=%s\n' "$CHECKS" "$FAILURES" "$SELF_CHECK_FAILURES"
[ "$FAILURES" -eq 0 ] && [ "$SELF_CHECK_FAILURES" -eq 0 ]
