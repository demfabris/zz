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
    maximum="$3"
    shift 3
    output_file="$HOME/positional-maximum-$label.out"
    error_file="$HOME/positional-maximum-$label.err"
    expected="command $canonical: too many arguments (need at most $maximum)"
    set +e
    main_client "$@" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    actual="$(cat "$error_file")"
    if [ "$status" -ne 1 ] || [ -s "$output_file" ] || [ "$actual" != "$expected" ]; then
        failed=1
    fi
}

input_path="$HOME/positional-maximum-input"
output_path="$HOME/positional-maximum-output"
printf 'load sentinel' >"$input_path"
printf 'save sentinel' >"$output_path"
main_client set-buffer -b source alpha
active_before="$(main_client display-message -p '#{pane_id}')"

probe choose-buffer choose-buffer 1 choose-buffer 'set-buffer -b callback fired' -t =missing
probe choose-tree choose-tree 1 choose-tree 'set-buffer -b callback fired' -t =missing
probe display-message display-message 1 display-message one -t =missing
probe display-panes display-panes 1 display-panes 'set-buffer -b callback fired' -t missing
probe load-buffer load-buffer 1 load-buffer "$input_path" -b loaded
probe save-buffer save-buffer 1 save-buffer "$output_path" -b source
probe select-pane select-pane 0 select-pane one -t =missing
probe set-buffer set-buffer 1 set-buffer alpha -n renamed

probe display-alias display-message 1 display one -t =missing
probe displayp-alias display-panes 1 displayp one -t missing
probe loadb-alias load-buffer 1 loadb "$input_path" -b alias-loaded
probe saveb-alias save-buffer 1 saveb "$output_path" -b source
probe selectp-alias select-pane 0 selectp one -t =missing
probe setb-alias set-buffer 1 setb alpha -n alias-renamed

active_after="$(main_client display-message -p '#{pane_id}')"
buffers="$(main_client list-buffers -F '#{buffer_name}=#{buffer_sample}')"
saved="$(cat "$output_path")"
if [ "$active_after" != "$active_before" ] || [ "$buffers" != 'source=alpha' ] ||
    [ "$saved" != 'save sentinel' ]; then
    failed=1
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g POSITIONAL_MAXIMUMS clean
fi
