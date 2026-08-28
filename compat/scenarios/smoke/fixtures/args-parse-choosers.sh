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

work="$HOME/args-parse-choosers-work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0

fail_check() {
    failed=1
    printf '%s\n' "$1" >>"$work/failures"
}

check_command() {
    label="$1"
    shift
    check_count=$((check_count + 1))
    if ! "$@"; then
        fail_check "$label"
    fi
}

expect_output() {
    label="$1"
    expected="$2"
    shift 2
    check_count=$((check_count + 1))
    set +e
    actual="$(main_client "$@" 2>/dev/null)"
    status=$?
    set -e
    if [ "$status" -ne 0 ] || [ "$actual" != "$expected" ]; then
        fail_check "$label"
    fi
}

expect_failure() {
    label="$1"
    shift
    check_count=$((check_count + 1))
    if main_client "$@" >/dev/null 2>&1; then
        fail_check "$label"
    fi
}

expect_error() {
    label="$1"
    expected="$2"
    shift 2
    output_file="$work/$label.out"
    error_file="$work/$label.err"
    expected_error_file="$work/$label.expected-err"
    check_count=$((check_count + 1))
    printf '%s\n' "$expected" >"$expected_error_file"
    set +e
    main_client "$@" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    if [ "$status" -ne 1 ] || [ -s "$output_file" ] ||
        ! cmp -s "$error_file" "$expected_error_file"; then
        fail_check "$label"
    fi
}

expect_environment() {
    label="$1"
    name="$2"
    expected_value="$3"
    check_count=$((check_count + 1))
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        actual="$(main_client show-environment -g "$name" 2>/dev/null || true)"
        if [ "$actual" = "$name=$expected_value" ]; then
            return
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    fail_check "$label"
}

expect_absent_environment() {
    label="$1"
    name="$2"
    check_count=$((check_count + 1))
    if main_client show-environment -g "$name" >/dev/null 2>&1; then
        fail_check "$label"
    fi
}

outer_tmux="${ZZ_SMOKE_TMUX_BIN:-$(command -v tmux)}"
outer_label="zzchooser-input-$$"
outer_client() {
    env -u TMUX -u TMUX_PANE "$outer_tmux" -L "$outer_label" "$@"
}

outer_socket=''
target_client=''
cleanup_outer() {
    outer_client kill-server >/dev/null 2>&1 || true
    if [ -n "$target_client" ]; then
        cleanup_attempt=0
        while [ "$cleanup_attempt" -lt 100 ]; do
            if ! main_client list-clients -F '#{client_name}' 2>/dev/null |
                grep -Fxq "$target_client"; then
                break
            fi
            cleanup_attempt=$((cleanup_attempt + 1))
            sleep 0.05
        done
    fi
    if [ -n "$outer_socket" ]; then
        rm -f -- "$outer_socket"
    fi
}
trap cleanup_outer EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

expect_screen_marker() {
    label="$1"
    marker="$2"
    check_count=$((check_count + 1))
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        screen="$(outer_client capture-pane -p -t input:0.0 2>/dev/null || true)"
        if printf '%s\n' "$screen" | grep -Fq -- "$marker"; then
            return
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    fail_check "$label"
}

expect_root_sync() {
    label="$1"
    trigger="${2:-}"
    check_count=$((check_count + 1))
    main_client set-environment -gu CHOOSER_SYNC >/dev/null 2>&1 || true
    if [ -n "$trigger" ] &&
        ! outer_client send-keys -t input:0.0 "$trigger"; then
        fail_check "$label"
        return
    fi
    if ! outer_client send-keys -t input:0.0 F12; then
        fail_check "$label"
        return
    fi
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        sync="$(main_client show-environment -g CHOOSER_SYNC 2>/dev/null || true)"
        state="$(main_client list-clients -F '#{client_session}|#{client_key_table}' 2>/dev/null || true)"
        if [ "$sync" = 'CHOOSER_SYNC=yes' ] && [ "$state" = 'w|root' ]; then
            return
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    fail_check "$label"
}

for alias in \
    'command-alias[90]=oldpick=set-option -g @TYPED' \
    'command-alias[91]=latepick=set-environment -g QUOTED_BEFORE'
do
    name=${alias%%=*}
    value=${alias#*=}
    if ! main_client set-option -s "$name" "$value"; then
        fail_check alias-setup
    fi
done

for name in \
    '@TYPED' \
    QUOTED_BEFORE \
    QUOTED_AFTER \
    EMPTY_RAN \
    CHOOSER_SYNC
do
    main_client set-environment -gu "$name" >/dev/null 2>&1 || true
done

main_client new-session -d -s chooser-target
main_client set-option -g display-time 60000

bindings="$work/bindings.conf"
printf '%s\n' \
    "bind-key -T root F1 choose-buffer -K '#{line}' { oldpick '%1:%%:%1:%%' }" \
    "bind-key -T root F2 choose-buffer -K '#{line}' 'latepick %%%'" \
    "bind-key -T root F3 choose-tree -s -K '#{line}' -O index 'set-option -p @TREE_SELECTED %1'" \
    "bind-key -T root F4 choose-buffer -K '#{line}' 'unknownchooser %1'" \
    "bind-key -T root F5 choose-buffer -K '#{line}' 'kill-pane -t missing-%1'" \
    "bind-key -T root F6 choose-buffer -K '#{line}' 'set-environment -g EMPTY_RAN yes'" \
    'bind-key -T root F12 set-environment -g CHOOSER_SYNC yes' \
    >"$bindings"
check_command binding-load main_client source-file "$bindings"

expect_error direct-buffer-arity \
    'command choose-buffer: too many arguments (need at most 1)' \
    choose-buffer -F format one two
expect_error direct-tree-arity \
    'command choose-tree: too many arguments (need at most 1)' \
    choose-tree -F format one two

typed_arity="$work/typed-arity.conf"
printf '%s\n' \
    'bind-key -T root F11 choose-buffer -F format { display-message one } { display-message two }' \
    >"$typed_arity"
expect_error typed-stored-arity \
    'command choose-buffer: too many arguments (need at most 1)' \
    source-file "$typed_arity"

attach_script="$work/attach-client.sh"
printf '%s\n' \
    '#!/bin/sh' \
    "if [ -n \"\${ZZ_SMOKE_ZZ_BIN:-}\" ]; then" \
    "  exec env -u TMUX -u TMUX_PANE \"\$ZZ_SMOKE_ZZ_BIN\" --socket \"\$ZZ_SMOKE_ZZ_SOCKET\" attach-session -t w" \
    'fi' \
    "exec env -u TMUX -u TMUX_PANE \"\$ZZ_SMOKE_TMUX_BIN\" -L \"\$ZZ_SMOKE_TMUX_LABEL\" attach-session -t w" \
    >"$attach_script"
if ! outer_client -f /dev/null new-session -d -x 160 -y 24 -s input \
    "sh '$attach_script'"; then
    fail_check chooser-client-start
fi
outer_socket="$(outer_client display-message -p '#{socket_path}' 2>/dev/null || true)"
attempt=0
while [ "$attempt" -lt 200 ] && [ -z "$target_client" ]; do
    target_client="$(main_client list-clients -F '#{client_name}' 2>/dev/null |
        head -n 1 || true)"
    attempt=$((attempt + 1))
    [ -n "$target_client" ] || sleep 0.05
done
if [ -z "$target_client" ]; then
    fail_check chooser-client-attach
fi

main_client set-buffer -b safe CHOOSER_TYPED_READY
outer_client send-keys -t input:0.0 F1
expect_screen_marker typed-open CHOOSER_TYPED_READY
main_client set-option -s 'command-alias[92]' \
    'set-option=set-environment'
outer_client send-keys -t input:0.0 0
expect_environment typed-fresh-parse @TYPED 'safe:safe:safe:%%'
expect_failure typed-option-absent show-options -gv @TYPED
expect_root_sync typed-close
main_client set -su 'command-alias[92]'
main_client delete-buffer -b safe

special='x"$;~'
main_client set-buffer -b "$special" CHOOSER_QUOTED_READY
outer_client send-keys -t input:0.0 F2
expect_screen_marker quoted-open CHOOSER_QUOTED_READY
main_client set-option -s 'command-alias[91]' \
    'latepick=set-environment -g QUOTED_AFTER'
outer_client send-keys -t input:0.0 0
expect_environment quoted-fresh-alias QUOTED_AFTER "$special"
expect_absent_environment quoted-old-alias QUOTED_BEFORE
expect_root_sync quoted-close
main_client delete-buffer -b "$special"

outer_client send-keys -t input:0.0 F3
expect_screen_marker tree-open chooser-target
outer_client send-keys -t input:0.0 1
expect_output tree-source-context '=chooser-target:' \
    display-message -p -t '=w:0.0' '#{@TREE_SELECTED}'
expect_output tree-target-context '' \
    display-message -p -t '=chooser-target:0.0' '#{@TREE_SELECTED}'
expect_root_sync tree-close

main_client set-buffer -b safe CHOOSER_ERROR_READY
outer_client send-keys -t input:0.0 F4
expect_screen_marker parse-error-open CHOOSER_ERROR_READY
outer_client send-keys -t input:0.0 0
expect_screen_marker parse-error-status 'Unknown comman'
expect_root_sync parse-error-close

outer_client send-keys -t input:0.0 F5
expect_screen_marker runtime-error-open CHOOSER_ERROR_READY
outer_client send-keys -t input:0.0 0
expect_screen_marker runtime-error-status "Can't find pan"
expect_root_sync runtime-error-close
main_client delete-buffer -b safe

expect_root_sync empty-buffer-noop F6
expect_absent_environment empty-template-not-run EMPTY_RAN
expect_output empty-buffer-list '' list-buffers -F '#{buffer_name}'
expect_output empty-pane-mode 0 display-message -p -t '=w:0.0' '#{pane_in_mode}'

cleanup_outer
main_client set-option -pu -t '=w:0.0' @TREE_SELECTED >/dev/null 2>&1 || true
main_client kill-session -t '=chooser-target' >/dev/null 2>&1 || true
for index in 90 91 92; do
    main_client set-option -su "command-alias[$index]" >/dev/null 2>&1 || true
done
main_client resize-window -t w -x 80 -y 23

if [ "$check_count" -ne 26 ]; then
    fail_check "check-count-$check_count"
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g ARGS_PARSE_CHOOSERS clean:26
else
    failure_labels="$(paste -sd, "$work/failures")"
    failure_side="${ZZ_SMOKE_CANARY:-missing-canary}"
    main_client set-environment -g ARGS_PARSE_CHOOSERS \
        "failed:$failure_side:$failure_labels"
    printf '%s:%s\n' "$failure_side" "$failure_labels" >&2
fi
