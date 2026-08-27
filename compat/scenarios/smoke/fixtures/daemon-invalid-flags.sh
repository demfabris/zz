#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

failed=0
probe() {
    name="$1"
    shift
    output="$HOME/daemon-invalid-$name.out"
    errors="$HOME/daemon-invalid-$name.err"
    set +e
    main_client "$@" >"$output" 2>"$errors"
    status=$?
    set -e
    if [ "$status" -eq 1 ] && [ ! -s "$output" ] &&
        grep -Fq -- "$name" "$errors" && grep -Fq -- '-G' "$errors"; then
        :
    else
        failed=1
    fi
}

probe set-buffer set-buffer -G sentinel
probe run-shell run-shell -C -G 'set-buffer callback-ran'
probe if-shell if-shell -F -G 1 'set-buffer callback-ran'
probe lock-server lock-server -G
probe wait-for wait-for -G -S daemon-invalid-flags

buffer_output="$HOME/daemon-invalid-buffers.out"
buffer_errors="$HOME/daemon-invalid-buffers.err"
set +e
main_client list-buffers -F '#{buffer_sample}' >"$buffer_output" 2>"$buffer_errors"
buffer_status=$?
set -e
if [ "$buffer_status" -ne 0 ] || [ -s "$buffer_output" ] || [ -s "$buffer_errors" ]; then
    failed=1
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DAEMON_INVALID_FLAGS clean
fi
