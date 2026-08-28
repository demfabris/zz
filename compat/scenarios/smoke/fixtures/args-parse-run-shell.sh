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

work="$HOME/args-parse-run-shell-work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0

probe() {
    label="$1"
    expected_status="$2"
    expected_output="$3"
    expected_error="$4"
    config="$work/$label.conf"
    check_count=$((check_count + 1))
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

typed_first="$work/typed-first.conf"
printf '%s\n' 'run-shell { display-message -p forbidden-first }' >"$typed_first"
probe typed-first 1 \
    "$typed_first:1: command run-shell: argument 1 must be \"string\"" ''

typed_second="$work/typed-second.conf"
printf '%s\n' "run-shell 'printf forbidden' { display-message -p forbidden-second }" >"$typed_second"
probe typed-second 1 \
    "$typed_second:1: command run-shell: argument 2 must be \"string\"" ''

late_command_flag="$work/late-command-flag.conf"
printf '%s\n' "run-shell 'printf forbidden' -C { display-message -p forbidden-third }" >"$late_command_flag"
probe late-command-flag 1 \
    "$late_command_flag:1: command run-shell: argument 3 must be \"string\"" ''

typed_target="$work/typed-target.conf"
printf '%s\n' "run-shell -t { display-message -p target } -C { display-message -p forbidden-target }" >"$typed_target"
probe typed-target 1 \
    "$typed_target:1: command run-shell: -t argument must be a string" ''

for option in c d s; do
    typed_option="$work/typed-$option.conf"
    printf '%s\n' "run-shell -C -$option { display-message -p option } { display-message -p forbidden-option }" >"$typed_option"
    probe "typed-$option" 1 \
        "$typed_option:1: command run-shell: -$option argument must be a string" ''
done

attached_delay="$work/attached-delay.conf"
printf '%s\n' 'run-shell -Cd0 { display-message -p attached-delay }' >"$attached_delay"
probe attached-delay 0 attached-delay ''

option_boundary="$work/option-boundary.conf"
printf '%s\n' 'run-shell -C -- { display-message -p option-boundary }' >"$option_boundary"
probe option-boundary 0 option-boundary ''

for boundary in attached-delay-value attached-cwd-value double-dash literal-dash; do
    boundary_file="$work/$boundary.conf"
    case "$boundary" in
        attached-delay-value)
            line='run-shell -d0C { display-message -p forbidden-boundary }'
            position=1
            ;;
        attached-cwd-value)
            line='run-shell -cC { display-message -p forbidden-boundary }'
            position=1
            ;;
        double-dash)
            line='run-shell -- -C { display-message -p forbidden-boundary }'
            position=2
            ;;
        literal-dash)
            line='run-shell - -C { display-message -p forbidden-boundary }'
            position=3
            ;;
    esac
    printf '%s\n' "$line" >"$boundary_file"
    probe "$boundary" 1 \
        "$boundary_file:1: command run-shell: argument $position must be \"string\"" ''
done

aliases="$work/aliases.conf"
main_client set-option -s command-alias[90] 'zzrun84=run-shell'
printf '%s\n' \
    'run-shell -C { display-message -p typed-canonical }' \
    'run -C { display-message -p typed-builtin }' \
    'run-s -C { display-message -p typed-prefix }' \
    'zzrun84 -C { display-message -p typed-user }' >"$aliases"
probe aliases 0 'typed-canonical
typed-builtin
typed-prefix
typed-user' ''

string_command="$work/string-command.conf"
printf '%s\n' "run-shell -C 'display-message -p string-command'" >"$string_command"
probe string-command 0 string-command ''

quoted_shell="$work/quoted-shell.conf"
printf '%s\n' 'run-shell "{ printf quoted-shell; }"' >"$quoted_shell"
probe quoted-shell 0 quoted-shell ''

quoted_command="$work/quoted-command.conf"
printf '%s\n' 'run-shell -C "{ display-message -p quoted-command }"' >"$quoted_command"
probe quoted-command 1 '' "$quoted_command:1: syntax error"

typed_ignored="$work/typed-ignored.conf"
printf '%s\n' 'run-shell -C { display-message -p typed-first } { display-message -p ignored-second }' >"$typed_ignored"
probe typed-ignored 0 typed-first ''

main_client set-environment -gu RUN_SHELL_BACKGROUND
background="$work/background.conf"
printf '%s\n' 'run-shell -bCd 0.05 { set-environment -g RUN_SHELL_BACKGROUND typed }' >"$background"
probe background 0 '' ''
deadline=100
while [ "$deadline" -gt 0 ]; do
    if [ "$(main_client show-environment -g RUN_SHELL_BACKGROUND 2>/dev/null || true)" = \
        'RUN_SHELL_BACKGROUND=typed' ]; then
        break
    fi
    sleep 0.02
    deadline=$((deadline - 1))
done
if [ "$deadline" -eq 0 ]; then
    failed=1
    printf '%s\n' background-effect >>"$work/failures"
fi

stored="$work/stored.conf"
printf '%s\n' \
    'bind-key -T prefix F11 run-shell -C { display-message -p stored }' \
    'bind-key -T prefix F11 run-shell { display-message -p forbidden }' >"$stored"
probe stored 1 '' 'command run-shell: argument 1 must be "string"'
stored_expected='F11=run-shell -C { display-message -p stored }'
if [ "$(main_client list-keys -T prefix -F '#{key_string}=#{key_command}' F11)" != \
    "$stored_expected" ]; then
    failed=1
    printf '%s\n' stored-command-kind >>"$work/failures"
fi

check_count=$((check_count + 1))
main_client set-environment -gu RUN_SHELL_CONTROL_OK
main_client set-environment -gu RUN_SHELL_CONTROL_FORBIDDEN
control_reject_raw="$work/control-reject.raw"
control_reject_error="$work/control-reject.err"
{
    printf '%s\n' 'run-shell { set-environment -g RUN_SHELL_CONTROL_FORBIDDEN yes }'
    printf '%s\n' 'detach-client'
} | control_client >"$control_reject_raw" 2>"$control_reject_error"
control_accept_raw="$work/control-accept.raw"
control_accept_error="$work/control-accept.err"
{
    printf '%s\n' 'run -C { set-environment -g RUN_SHELL_CONTROL_OK typed }'
    printf '%s\n' 'detach-client'
} | control_client >"$control_accept_raw" 2>"$control_accept_error"
if ! grep -Fq 'command run-shell: argument 1 must be "string"' "$control_reject_raw" ||
    [ -s "$control_reject_error" ] || [ -s "$control_accept_error" ] ||
    [ "$(main_client show-environment -g RUN_SHELL_CONTROL_OK)" != \
        'RUN_SHELL_CONTROL_OK=typed' ]; then
    failed=1
    printf '%s\n' control-typed-arguments >>"$work/failures"
fi
if main_client show-environment -g RUN_SHELL_CONTROL_FORBIDDEN >/dev/null 2>&1; then
    failed=1
    printf '%s\n' control-typed-effect >>"$work/failures"
fi

if [ "$failed" -eq 0 ] && [ "$check_count" -eq 21 ]; then
    main_client set-environment -g ARGS_PARSE_RUN_SHELL clean:21
fi
