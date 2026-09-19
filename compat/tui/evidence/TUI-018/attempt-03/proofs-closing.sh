#!/usr/bin/env bash
E=compat/tui/evidence/TUI-018/attempt-03
FROZEN="$PWD/target/c11-alias-proof/zz"
failed=0
run_proof() {
  local label="$1"
  shift
  bash "$E/run.sh" "$label" env ZZ_BIN="$FROZEN" ZZ_COMPAT_ZZ="$FROZEN" "$@" || failed=1
}
run_proof streams-closing-57 compat/tui-command-streams.sh
run_proof streams-self-check-57 compat/tui-command-streams.sh --self-check
run_proof client-commands-57 compat/tui-client-commands.sh
run_proof client-self-check-57 compat/tui-client-commands.sh --self-check
run_proof attached-client-57 compat/attached-client.sh
run_proof verify-claims-57 python3 compat/tui/verify-claims.py --run TUI-018 --zz "$FROZEN"
exit "$failed"
