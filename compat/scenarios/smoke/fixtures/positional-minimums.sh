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
    label="$1"
    canonical="$2"
    minimum="$3"
    shift 3
    output_file="$HOME/positional-minimum-$label.out"
    error_file="$HOME/positional-minimum-$label.err"
    expected="command $canonical: too few arguments (need at least $minimum)"
    set +e
    main_client "$@" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    actual="$(cat "$error_file")"
    if [ "$status" -ne 1 ] || [ -s "$output_file" ] || [ "$actual" != "$expected" ]; then
        failed=1
    fi
}

output_path="$HOME/positional-minimum-output"
printf 'save sentinel' >"$output_path"
main_client set-buffer -b source alpha
active_before="$(main_client display-message -p '#{pane_id}')"
buffers_before="$(main_client list-buffers -F '#{buffer_name}=#{buffer_sample}')"

probe bind-key bind-key 1 bind-key
probe confirm-before confirm-before 1 confirm-before -t missing
probe display-menu display-menu 1 display-menu -c missing -t =missing
probe find-window find-window 1 find-window -t =missing
probe if-shell if-shell 2 if-shell -t =missing condition
probe load-buffer load-buffer 1 load-buffer -t missing
probe rename-session rename-session 1 rename-session -t =missing
probe rename-window rename-window 1 rename-window -t =missing
probe save-buffer save-buffer 1 save-buffer
probe set-environment set-environment 1 set-environment -t =missing
probe set-option set-option 1 set-option -t =missing
probe set-window-option set-window-option 1 set-window-option -t =missing
probe source-file source-file 1 source-file -t =missing
probe wait-for wait-for 1 wait-for

probe bind-alias bind-key 1 bind
probe confirm-alias confirm-before 1 confirm -t missing
probe menu-alias display-menu 1 menu -c missing -t =missing
probe findw-alias find-window 1 findw -t =missing
probe if-alias if-shell 2 if -t =missing condition
probe loadb-alias load-buffer 1 loadb -t missing
probe rename-alias rename-session 1 rename -t =missing
probe renamew-alias rename-window 1 renamew -t =missing
probe saveb-alias save-buffer 1 saveb
probe setenv-alias set-environment 1 setenv -t =missing
probe set-alias set-option 1 set -t =missing
probe setw-alias set-window-option 1 setw -t =missing
probe source-alias source-file 1 source -t =missing
probe wait-alias wait-for 1 wait

active_after="$(main_client display-message -p '#{pane_id}')"
buffers_after="$(main_client list-buffers -F '#{buffer_name}=#{buffer_sample}')"
saved="$(cat "$output_path")"
if [ "$active_after" != "$active_before" ] || [ "$buffers_after" != "$buffers_before" ] ||
    [ "$saved" != 'save sentinel' ]; then
    failed=1
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g POSITIONAL_MINIMUMS clean
fi
