#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
OUT="${1:-$HOME/.cache/zz-catchup/logs/suite}"
mkdir -p "$OUT"
export PATH="$PWD/compat/catchup/bin:$PATH" LANG=en_US.UTF-8
C=compat/catchup/cargo.sh
SUMMARY="$OUT/summary.txt"
: > "$SUMMARY"
step() {
  local name="$1"; shift
  local start=$SECONDS
  timeout "${STEP_TIMEOUT:-9000}" "$@" > "$OUT/$name.log" 2>&1
  local rc=$?
  printf '%-34s exit %-3s %5ss\n' "$name" "$rc" "$((SECONDS - start))" >> "$SUMMARY"
}
echo "main $(git rev-parse --short HEAD) $(date -Is)" >> "$SUMMARY"
step clippy-workspace $C clippy --workspace --all-targets --all-features -- -D warnings
step clippy-daemon-no-agent $C clippy -p zz-daemon -p zz-daemon-client --no-default-features --all-targets -- -D warnings
step ci-test $C ci-test --no-fail-fast
step compat-check just compat check
step build-zz-cli $C build -p zz-cli --bin zz_cli
mkdir -p target/catchup-scratch
cp target/debug/zz_cli target/catchup-scratch/zz_cli
export ZZ_COMPAT_ZZ="$PWD/target/catchup-scratch/zz_cli" ZZ_COMPAT_TMUX="$PWD/compat/.cache/tmux-src/tmux" ZZ_COMPAT_CORPUS="$PWD/compat/.cache/plugins"
export ZZ_BIN="$ZZ_COMPAT_ZZ" TMUX_BIN="$ZZ_COMPAT_TMUX"
step run-sh-attached compat/run.sh --strict-geometry --attached-client
for f in compat/tui-*.sh; do
  STEP_TIMEOUT=3600 step "$(basename "$f" .sh)" "$f"
done
echo "SUITE-DONE $(date -Is)" >> "$SUMMARY"
