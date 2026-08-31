#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

entry=compat/scenarios/source-hook-cwd-sourced/a/entry.conf
relative=compat/scenarios/source-hook-cwd-sourced/leaf.conf
work="$HOME/source-hook-cwd-sourced-work-$side"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0

record_failure() {
    failed=1
    echo "$1" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1"
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    main_client set-hook -gu after-display-message >/dev/null 2>&1
    main_client set-option -gu @source_hook_cwd_sourced >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

mkdir -p "$HOME/$(dirname "$relative")"
printf 'set-option -g @source_hook_cwd_sourced daemon-home-decoy\n' \
    >"$HOME/$relative"

main_client set-option -g @source_hook_cwd_sourced unset
main_client -C source-file "$entry" </dev/null >"$work/control" 2>&1

check_equal selected-cwd client-root \
    "$(main_client show-options -gv @source_hook_cwd_sourced)"
check_equal control-body SOURCED_HOOK \
    "$(grep -v '^%' "$work/control" | tr '\n' ' ' | sed 's/ *$//')"
check_equal control-errors 0 \
    "$(grep -c '^%error ' "$work/control" | tr -d '[:space:]')"
check_equal control-frames 6:6 \
    "$(grep -c '^%begin ' "$work/control" | tr -d '[:space:]'):$(grep -c '^%end ' "$work/control" | tr -d '[:space:]')"
check_equal control-flags 0 \
    "$(awk '/^%(begin|end|error) /{ if ($4 != 0) bad++ } END { print bad + 0 }' \
        "$work/control")"
check_equal control-exit '%exit' "$(tail -n 1 "$work/control")"

if [ "$check_count" -ne 6 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g SOURCE_HOOK_CWD_SOURCED clean:6
else
    sed "s/^/source-hook-cwd-sourced-$side: /" "$work/failures"
fi
