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

work="$HOME/buffer-missing-target-error-work-$side"
transcript="$HOME/buffer-missing-target-error-$side.txt"
rm -rf "$work"
mkdir -p "$work"
: >"$transcript"

drop_every_buffer() {
    main_client list-buffers -F '#{buffer_name}' 2>/dev/null |
        while IFS= read -r existing; do
            [ -n "$existing" ] || continue
            main_client delete-buffer -b "$existing" >/dev/null 2>&1 || true
        done
}

probe() {
    label=$1
    shift
    status=0
    message="$(main_client "$@" 2>&1 >/dev/null)" || status=$?
    printf '%s status=%s message=%s\n' "$label" "$status" "$message" >>"$transcript"
}

drop_every_buffer
printf 'buffers-now=[%s]\n' "$(main_client list-buffers -F '#{buffer_name}' | tr '\n' ',')" \
    >>"$transcript"

probe empty-paste-named paste-buffer -b nosuch
probe empty-save-named save-buffer -b nosuch "$work/out"
probe empty-show-named show-buffer -b nosuch
probe empty-delete-named delete-buffer -b nosuch
probe empty-rename-named set-buffer -n newname -b nosuch
probe empty-save-top save-buffer "$work/out"
probe empty-show-top show-buffer
probe empty-delete-top delete-buffer
probe empty-rename-top set-buffer -n newname
probe empty-paste-top paste-buffer

main_client set-buffer -b present payload
printf 'buffers-now=[%s]\n' "$(main_client list-buffers -F '#{buffer_name}' | tr '\n' ',')" \
    >>"$transcript"

probe stocked-paste-named paste-buffer -b nosuch
probe stocked-save-named save-buffer -b nosuch "$work/out"
probe stocked-show-named show-buffer -b nosuch
probe stocked-delete-named delete-buffer -b nosuch
probe stocked-rename-named set-buffer -n newname -b nosuch
probe stocked-save-top save-buffer "$work/out"
probe stocked-show-top show-buffer
probe stocked-delete-top delete-buffer
probe stocked-rename-top set-buffer -n newname

drop_every_buffer
rm -rf "$work"

main_client load-buffer -b transcript "$transcript"
