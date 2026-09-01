#!/bin/sh
set -eu

byte_root="$HOME/config-non-utf8-file-bytes"
rm -rf "$byte_root"
mkdir -p "$byte_root"

printf '\377' >"$byte_root/isolated.conf"
printf '\377set-environment -g CONFIG_BYTE_CONTINUED continued\n' \
    >"$byte_root/continued.conf"
: >"$byte_root/empty.conf"
printf 'display-message -p ROOT_BEFORE\nif-shell -F 1 "source-file %s"\ndisplay-message -p ROOT_AFTER\n' \
    "$byte_root/isolated.conf" >"$byte_root/root.conf"

flatten() {
    sed "s|$byte_root/||g" "$1" | tr '\n' '~'
}

run_source() {
    if tmux source-file -v "$1" >"$byte_root/out" 2>"$byte_root/err"; then
        source_rc=0
    else
        source_rc=$?
    fi
}

probe() {
    run_source "$2"
    tmux set-environment -g "$1" \
        "rc=$source_rc out=$(flatten "$byte_root/out") err=$(flatten "$byte_root/err")"
}

probe CONFIG_BYTE_DIRECT "$byte_root/isolated.conf"
probe CONFIG_BYTE_EMPTY "$byte_root/empty.conf"
probe CONFIG_BYTE_CONTINUED_SOURCE "$byte_root/continued.conf"
probe CONFIG_BYTE_IF_SHELL "$byte_root/root.conf"

if continued_marker="$(tmux show-environment -g CONFIG_BYTE_CONTINUED 2>&1)"; then
    continued_rc=0
else
    continued_rc=$?
fi
tmux set-environment -g CONFIG_BYTE_CONTINUED_MARKER \
    "rc=$continued_rc value=$continued_marker"

matrix=''
matrix_case() {
    printf "$2" >"$byte_root/case.conf"
    run_source "$byte_root/case.conf"
    matrix="$matrix|$1:rc=$source_rc:out=$(flatten "$byte_root/out"):err=$(flatten "$byte_root/err")"
}

matrix_case isolated-ff '\377'
matrix_case embedded-ff 'display-message -p before\377after\n'
matrix_case trailing-ff 'display-message -p before\377\n'
matrix_case boundary-ff 'display-message -p before \377display-message -p after\n'
matrix_case second-boundary-ff '\377display-message -p first\n\377display-message -p second\ndisplay-message -p third\n'
matrix_case comment-ff '# ignored\377display-message -p escaped\n'
matrix_case even-backslash-ff 'display-message -p before\\\\\377after\n'
matrix_case unicode-eof 'display-message -p \\u12\37734\n'
matrix_case second-eof-incomplete '\377display-message -p after'
matrix_case block-hard-eof 'display-message -p before\nif-shell true { display-message -p one \377display-message -p two \377display-message -p three\n }\ndisplay-message -p after\n'
matrix_case block-quote-ff 'bind-key x { send-keys '"'"'before\377after'"'"' }\n'
tmux set-environment -g CONFIG_BYTE_MATRIX "$matrix"

startup_conf="$byte_root/startup.conf"
printf '\377set-environment -g CONFIG_BYTE_STARTUP set\n' >"$startup_conf"
if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    startup_socket="/tmp/zzcbfb-$$.sock"
    startup_client="$ZZ_SMOKE_ZZ_BIN --socket $startup_socket"
else
    startup_client="$ZZ_SMOKE_TMUX_BIN -L zzcbfb-$$"
fi
if $startup_client -f "$startup_conf" new-session -d \
    >"$byte_root/startup.out" 2>"$byte_root/startup.err"; then
    startup_rc=0
else
    startup_rc=$?
fi
if $startup_client show-environment -g CONFIG_BYTE_STARTUP \
    >"$byte_root/startup.marker" 2>"$byte_root/startup.marker.err"; then
    startup_marker_rc=0
else
    startup_marker_rc=$?
fi
$startup_client kill-server >/dev/null 2>&1 || true
case "${startup_socket:-}" in
/tmp/zzcbfb-*.sock) rm -f -- "${startup_socket:-}" ;;
esac
tmux set-environment -g CONFIG_BYTE_STARTUP_RESULT \
    "rc=$startup_rc out=$(flatten "$byte_root/startup.out") err=$(flatten "$byte_root/startup.err") marker=$startup_marker_rc:$(flatten "$byte_root/startup.marker"):$(flatten "$byte_root/startup.marker.err")"
