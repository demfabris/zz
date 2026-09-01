#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    set -- --socket "$ZZ_SMOKE_ZZ_SOCKET"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    set -- -L "$ZZ_SMOKE_TMUX_LABEL"
fi
prefix_args="$*"
main_client() {
    # shellcheck disable=SC2086
    "$binary" $prefix_args "$@"
}

work="$HOME/buffer-client-file-load-work-$side"
rm -rf "$work"
mkdir -p "$work/first" "$work/second" "$work/second/nested" "$work/decoy-home"
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
    for name in first second nested tilde absolute; do
        main_client delete-buffer -b "$name" >/dev/null 2>&1
    done
    rm -f "$HOME/buffer-client-file-tilde.txt"
    exit "$cleanup_status"
}
trap cleanup EXIT

# Every probe runs the CLI as a command client from its own directory, with the
# harness environment scrubbed the way the differential scrubs it elsewhere.
probe_client() {
    directory="$1"
    home="$2"
    shift 2
    # shellcheck disable=SC2086
    (
        cd "$directory" &&
            env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
                HOME="$home" PWD="$directory" \
                "$binary" $prefix_args "$@"
    )
}

printf 'first directory payload' >"$work/first/shared.txt"
printf 'second directory payload' >"$work/second/shared.txt"
printf 'nested payload' >"$work/second/nested/deep.txt"
printf 'server home payload' >"$HOME/buffer-client-file-tilde.txt"
printf 'client home payload' >"$work/decoy-home/buffer-client-file-tilde.txt"
printf 'absolute payload' >"$work/absolute.txt"

probe_client "$work/first" "$HOME" load-buffer -b first shared.txt
check_equal first 'first directory payload' "$(main_client show-buffer -b first)"

probe_client "$work/second" "$HOME" load-buffer -b second shared.txt
check_equal second 'second directory payload' "$(main_client show-buffer -b second)"

probe_client "$work/second" "$HOME" load-buffer -b nested nested/deep.txt
check_equal nested 'nested payload' "$(main_client show-buffer -b nested)"

probe_client "$work/first" "$work/decoy-home" \
    load-buffer -b tilde '~/buffer-client-file-tilde.txt'
check_equal tilde 'server home payload' "$(main_client show-buffer -b tilde)"

probe_client "$work/first" "$HOME" load-buffer -b absolute "$work/absolute.txt"
check_equal absolute 'absolute payload' "$(main_client show-buffer -b absolute)"

missing_status=0
missing_error="$(probe_client "$work/first" "$HOME" \
    load-buffer -b missing nope.txt 2>&1 >/dev/null)" || missing_status=$?
check_equal missing-status 1 "$missing_status"
check_equal missing-error \
    "No such file or directory: $work/first/nope.txt" "$missing_error"

if [ "$check_count" -ne 7 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g BUFFER_CLIENT_FILE_LOAD clean:7
else
    sed "s/^/buffer-client-file-load-$side: /" "$work/failures"
fi
