set -eu
side="$1"
root="$PWD"
evidence="$root/compat/tui/evidence/TUI-017/attempt-05"
if [ "$side" = candidate ]; then
    binary="$root/target/capture-12-final-proof/zz"
else
    binary="$root/target/capture-12-final-main-build/debug/zz"
    cd "$root/target/capture-12-final-main-src"
fi
for iteration in 1 2 3; do
    rc=0
    compat/diff-scenario.sh --strict-geometry compat/scenarios/smoke/status-background-jobs.txt "$binary" "$root/compat/.cache/tmux-src/tmux" >"$evidence/status-$side-$iteration.txt" 2>&1 || rc=$?
    printf '\nexit_status: %s\n' "$rc" >>"$evidence/status-$side-$iteration.txt"
    cp compat/results/smoke/status-background-jobs.log "$evidence/status-$side-$iteration-transcript.txt"
    printf '%s repeat %s: exit %s\n' "$side" "$iteration" "$rc"
done
