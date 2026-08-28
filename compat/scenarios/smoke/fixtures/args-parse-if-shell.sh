#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" \
            -C attach-session -t w
    }
else
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" \
            -C attach-session -t w
    }
fi

work="$HOME/args-parse-if-shell-work"
mkdir -p "$work"
: >"$work/failures"
failed=0
probe_count=0

probe() {
    label="$1"
    expected_status="$2"
    expected_output="$3"
    expected_error="$4"
    config="$work/$label.conf"
    probe_count=$((probe_count + 1))
    output_file="$work/$label.out"
    error_file="$work/$label.err"
    expected_output_file="$work/$label.expected-out"
    expected_error_file="$work/$label.expected-err"
    if [ -n "$expected_output" ]; then
        printf '%s\n' "$expected_output" >"$expected_output_file"
    else
        : >"$expected_output_file"
    fi
    if [ -n "$expected_error" ]; then
        printf '%s\n' "$expected_error" >"$expected_error_file"
    else
        : >"$expected_error_file"
    fi
    set +e
    main_client source-file "$config" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    if [ "$status" -ne "$expected_status" ] ||
        ! cmp -s "$output_file" "$expected_output_file" ||
        ! cmp -s "$error_file" "$expected_error_file"; then
        failed=1
        printf '%s\n' "$label" >>"$work/failures"
    fi
}

typed_condition="$work/typed-condition.conf"
printf '%s\n' "if-shell -F { display-message -p condition } 'set-environment -g IF_SHELL_FORBIDDEN yes'" >"$typed_condition"
main_client set-environment -gu IF_SHELL_FORBIDDEN
probe typed-condition 1 \
    "$typed_condition:1: command if-shell: argument 1 must be \"string\"" ''
set +e
main_client show-environment -g IF_SHELL_FORBIDDEN >"$work/marker.out" 2>"$work/marker.err"
marker_status=$?
set -e
printf '%s\n' 'unknown variable: IF_SHELL_FORBIDDEN' >"$work/marker.expected"
if [ "$marker_status" -ne 1 ] || [ -s "$work/marker.out" ] ||
    ! cmp -s "$work/marker.err" "$work/marker.expected"; then
    failed=1
    printf '%s\n' typed-condition-effect >>"$work/failures"
fi

typed_true="$work/typed-true.conf"
printf '%s\n' 'if-shell -F 1 { display-message -p typed-true }' >"$typed_true"
probe typed-true 0 typed-true ''

typed_false="$work/typed-false.conf"
printf '%s\n' "if-shell -F 0 'display-message -p missed' { display-message -p typed-false }" >"$typed_false"
probe typed-false 0 typed-false ''

string_true="$work/string-true.conf"
printf '%s\n' "if-shell -F 1 'display-message -p string-true'" >"$string_true"
probe string-true 0 string-true ''

typed_shell="$work/typed-shell.conf"
printf '%s\n' "if-shell 'true' { display-message -p typed-shell }" >"$typed_shell"
probe typed-shell 0 typed-shell ''

quoted_brace="$work/quoted-brace.conf"
printf '%s\n' 'if-shell -F 1 "{ display-message -p quoted }"' >"$quoted_brace"
probe quoted-brace 1 '' "$quoted_brace:1: syntax error"

typed_fourth="$work/typed-fourth.conf"
printf '%s\n' "if-shell -F 1 'display-message -p first' 'display-message -p second' { display-message -p fourth }" >"$typed_fourth"
probe typed-fourth 1 \
    "$typed_fourth:1: command if-shell: argument 4 must be \"string\"" ''

typed_target="$work/typed-target.conf"
printf '%s\n' "if-shell -t { display-message -p target } -F 1 'display-message -p branch'" >"$typed_target"
probe typed-target 1 \
    "$typed_target:1: command if-shell: -t argument must be a string" ''

stored_valid="$work/stored-valid.conf"
printf '%s\n' 'bind-key -T prefix F12 if-shell -F 1 { display-message -p true } { display-message -p false }' >"$stored_valid"
probe stored-valid 0 '' ''

stored_invalid="$work/stored-invalid.conf"
printf '%s\n' "bind-key -T prefix F12 if-shell -F { display-message condition } 'display-message branch'" >"$stored_invalid"
probe stored-invalid 1 '' \
    'command if-shell: argument 1 must be "string"'
stored_expected='F12=if-shell -F 1 { display-message -p true } { display-message -p false }'
if [ "$(main_client list-keys -T prefix -F '#{key_string}=#{key_command}' F12)" != "$stored_expected" ]; then
    failed=1
    printf '%s\n' stored-command-kind >>"$work/failures"
fi

main_client set-environment -gu IF_SHELL_CONTROL_OK
main_client set-environment -gu IF_SHELL_CONTROL_FORBIDDEN
control_reject_raw="$work/control-reject.raw"
control_reject_error="$work/control-reject.err"
printf '%s\n' \
    'if-shell -F { display-message -p condition } { set-environment -g IF_SHELL_CONTROL_FORBIDDEN yes }' |
    control_client >"$control_reject_raw" 2>"$control_reject_error"
control_accept_raw="$work/control-accept.raw"
control_accept_error="$work/control-accept.err"
{
    printf '%s\n' 'if -F 1 { set-environment -g IF_SHELL_CONTROL_OK typed }'
    printf '%s\n' 'detach-client'
} | control_client >"$control_accept_raw" 2>"$control_accept_error"
if ! grep -Fq 'command if-shell: argument 1 must be "string"' "$control_reject_raw" ||
    [ -s "$control_reject_error" ] || [ -s "$control_accept_error" ] ||
    [ "$(main_client show-environment -g IF_SHELL_CONTROL_OK)" != 'IF_SHELL_CONTROL_OK=typed' ]; then
    failed=1
    printf '%s\n' control-typed-arguments >>"$work/failures"
fi
if main_client show-environment -g IF_SHELL_CONTROL_FORBIDDEN >/dev/null 2>&1; then
    failed=1
    printf '%s\n' control-typed-condition-effect >>"$work/failures"
fi

if [ "$failed" -eq 0 ] && [ "$probe_count" -eq 10 ]; then
    main_client set-environment -g ARGS_PARSE_IF_SHELL clean:12
fi
