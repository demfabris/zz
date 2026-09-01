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

work="$HOME/buffer-client-file-save-work-$side"
rm -rf "$work"
mkdir -p "$work/first" "$work/second" "$work/decoy-home"
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
    main_client delete-buffer -b saved >/dev/null 2>&1
    rm -f "$HOME/buffer-client-file-saved-tilde.txt"
    exit "$cleanup_status"
}
trap cleanup EXIT

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

file_text() {
    if [ -f "$1" ]; then
        cat "$1"
    else
        printf 'absent'
    fi
}

printf 'untouched' >"$work/second/out.txt"

main_client set-buffer -b saved 'first payload'
probe_client "$work/first" "$HOME" save-buffer -b saved out.txt
check_equal relative 'first payload' "$(file_text "$work/first/out.txt")"
check_equal untouched untouched "$(file_text "$work/second/out.txt")"

main_client set-buffer -b saved 'appended payload'
probe_client "$work/first" "$HOME" save-buffer -a -b saved out.txt
check_equal appended 'first payloadappended payload' "$(file_text "$work/first/out.txt")"

probe_client "$work/first" "$HOME" save-buffer -b saved out.txt
check_equal truncated 'appended payload' "$(file_text "$work/first/out.txt")"

probe_client "$work/first" "$work/decoy-home" \
    save-buffer -b saved '~/buffer-client-file-saved-tilde.txt'
check_equal tilde 'appended payload' \
    "$(file_text "$HOME/buffer-client-file-saved-tilde.txt")"
check_equal tilde-decoy absent \
    "$(file_text "$work/decoy-home/buffer-client-file-saved-tilde.txt")"

probe_client "$work/first" "$HOME" save-buffer -b saved "$work/second/absolute.txt"
check_equal absolute 'appended payload' "$(file_text "$work/second/absolute.txt")"

missing_status=0
missing_error="$(probe_client "$work/first" "$HOME" \
    save-buffer -b saved nodir/out.txt 2>&1 >/dev/null)" || missing_status=$?
check_equal missing-status 1 "$missing_status"
check_equal missing-error \
    "No such file or directory: $work/first/nodir/out.txt" "$missing_error"

if [ "$check_count" -ne 9 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g BUFFER_CLIENT_FILE_SAVE clean:9
else
    sed "s/^/buffer-client-file-save-$side: /" "$work/failures"
fi
