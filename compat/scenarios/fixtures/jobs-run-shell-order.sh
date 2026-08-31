#!/bin/sh
set -eu

export LC_ALL=C

case "${1:-}" in
record)
    printf '%s|%s' "${ORDER_V:-unset}" "${TERM:-unset}" >"$2"
    exit 0
    ;;
sequence)
    if [ -s "$2" ]; then
        printf 'done' >"$3"
    else
        printf 'pending' >"$3"
    fi
    exit 0
    ;;
esac

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

script="$HOME/jobs-run-shell-order.sh"
work="$HOME/jobs-run-shell-order-work"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures-$side"
failed=0
check_count=0
default_terminal=""

record_failure() {
    failed=1
    echo "$1" >>"$work/failures-$side"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1"
    fi
}

wait_for_marker() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ -s "$1" ]; then
            cat "$1"
            return
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    printf 'timeout'
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    main_client set-environment -gu ORDER_V >/dev/null 2>&1
    if [ -n "$default_terminal" ]; then
        main_client set-option -g default-terminal "$default_terminal" \
            >/dev/null 2>&1
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

default_terminal="$(main_client show-options -gv default-terminal)"

background_group() {
    label=$1
    delay_flag=$2
    marker="$work/$label-$side"
    config="$work/$label-$side.conf"
    {
        echo "set-environment -g ORDER_V ${label}-before"
        echo "set-option -g default-terminal ${label}-before-term"
        echo "run-shell -b $delay_flag \"sh '$script' record '$marker'\""
        echo "set-environment -g ORDER_V ${label}-after"
        echo "set-option -g default-terminal ${label}-after-term"
    } >"$config"
    main_client source-file "$config"
    check_equal "$label" "${label}-after|${label}-after-term" \
        "$(wait_for_marker "$marker")"
}

foreground_group() {
    label=$1
    delay_flag=$2
    marker="$work/$label-$side"
    order="$work/$label-order-$side"
    config="$work/$label-$side.conf"
    {
        echo "set-environment -g ORDER_V ${label}-before"
        echo "set-option -g default-terminal ${label}-before-term"
        echo "run-shell $delay_flag \"sh '$script' record '$marker'\""
        echo "run-shell \"sh '$script' sequence '$marker' '$order'\""
        echo "set-environment -g ORDER_V ${label}-after"
        echo "set-option -g default-terminal ${label}-after-term"
    } >"$config"
    main_client source-file "$config"
    check_equal "$label" "${label}-before|${label}-before-term" \
        "$(wait_for_marker "$marker")"
    check_equal "$label-order" done "$(wait_for_marker "$order")"
    check_equal "$label-drained" "ORDER_V=${label}-after" \
        "$(main_client show-environment -g ORDER_V)"
}

background_group immediate-background ""
background_group zero-background "-d 0"
foreground_group immediate-foreground ""
foreground_group zero-foreground "-d 0"

if [ "$check_count" -ne 8 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g JOBS_RUN_SHELL_ORDER clean:8
else
    sed "s/^/jobs-run-shell-order-$side: /" "$work/failures-$side"
fi
