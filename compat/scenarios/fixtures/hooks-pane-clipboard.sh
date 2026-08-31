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

work="$HOME/hooks-pane-clipboard-work-$side"
rm -rf "$work"
mkdir -p "$work"
ticks="$work/ticks"
: >"$ticks"
: >"$work/failures"
failed=0
check_count=0
set_clipboard=""
expected=""

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
    main_client set-hook -gu pane-set-clipboard >/dev/null 2>&1
    if [ -n "$set_clipboard" ]; then
        main_client set-option -s set-clipboard "$set_clipboard" >/dev/null 2>&1
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

cat >"$work/emit.sh" <<'EMIT'
#!/bin/sh
printf '\033]52;c;%s\007' "$1"
printf 'EMITTED-%s\n' "$1"
sleep 300
EMIT

cat >"$work/tick.sh" <<'TICK'
#!/bin/sh
printf '%s\n' "$2" >>"$1"
TICK

set_clipboard="$(main_client show-options -sv set-clipboard)"
main_client set-hook -g pane-set-clipboard \
    "run-shell \"sh '$work/tick.sh' '$ticks' '#{hook}|#{hook_pane}|#{hook_client}'\""

emit_case() {
    payload=$1
    pane="$(main_client split-window -d -P -F '#{pane_id}' \
        "sh '$work/emit.sh' $payload")"
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if main_client capture-pane -p -t "$pane" |
            grep -q "EMITTED-$payload"; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    if [ "$attempt" -ge 200 ]; then
        record_failure "emit-$payload"
    fi
    printf '%s' "$pane"
}

expect_tick() {
    payload=$1
    pane="$(emit_case "$payload")"
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if grep -q "|$pane|" "$ticks"; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    expected="$expected pane-set-clipboard|$pane|"
    main_client kill-pane -t "$pane"
}

expect_silence() {
    payload=$1
    pane="$(emit_case "$payload")"
    main_client kill-pane -t "$pane"
}

main_client set-option -s set-clipboard external
expect_silence ZXh0ZXJuYWw=
main_client set-option -s set-clipboard on
expect_tick b25l
expect_tick dHdv
main_client set-option -s set-clipboard off
expect_silence b2Zm
main_client set-option -s set-clipboard on
expect_tick dGhyZWU=

check_equal emissions "$(echo $expected)" "$(tr '\n' ' ' <"$ticks" | sed 's/ *$//')"
check_equal buffers 3 "$(main_client list-buffers | wc -l | tr -d '[:space:]')"
check_equal panes 1 \
    "$(main_client list-panes -t w:0 | wc -l | tr -d '[:space:]')"

if [ "$check_count" -ne 3 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g HOOKS_PANE_CLIPBOARD clean:3
else
    sed "s/^/hooks-pane-clipboard-$side: /" "$work/failures"
fi
