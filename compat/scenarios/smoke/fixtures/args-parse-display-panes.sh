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

work="$HOME/args-parse-display-panes-work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0

fail_check() {
    failed=1
    printf '%s\n' "$1" >>"$work/failures"
}

probe() {
    label="$1"
    expected_status="$2"
    expected_output="$3"
    expected_error="$4"
    config="$work/$label.conf"
    output_file="$work/$label.out"
    error_file="$work/$label.err"
    expected_output_file="$work/$label.expected-out"
    expected_error_file="$work/$label.expected-err"
    check_count=$((check_count + 1))
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
        fail_check "$label"
    fi
}

expect_key() {
    label="$1"
    key="$2"
    expected="$3"
    check_count=$((check_count + 1))
    set +e
    actual="$(main_client list-keys -T zzpanes10i \
        -F '#{key_string}=#{key_command}' "$key")"
    status=$?
    set -e
    if [ "$status" -ne 0 ] || [ "$actual" != "$expected" ]; then
        fail_check "$label"
    fi
}

runtime_probe() {
    label="$1"
    expected_status="$2"
    expected_error="$3"
    shift 3
    output_file="$work/runtime-$label.out"
    error_file="$work/runtime-$label.err"
    expected_error_file="$work/runtime-$label.expected-err"
    check_count=$((check_count + 1))
    printf '%s\n' "$expected_error" >"$expected_error_file"
    set +e
    main_client "$@" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    if [ "$status" -ne "$expected_status" ] || [ -s "$output_file" ] ||
        ! cmp -s "$error_file" "$expected_error_file"; then
        fail_check "runtime-$label"
    fi
}

main_client bind-key -T zzpanes10i F12 display-message -p preserved

typed_duration="$work/typed-duration.conf"
printf '%s\n' \
    'bind-key -T zzpanes10i F12 display-panes -d { display-message -p duration }' \
    >"$typed_duration"
probe typed-duration 1 '' \
    'command display-panes: -d argument must be a string'
expect_key typed-duration-preserved F12 'F12=display-message -p preserved'

typed_target="$work/typed-target.conf"
printf '%s\n' \
    'bind-key -T zzpanes10i F12 display-panes -t { display-message -p target }' \
    >"$typed_target"
probe typed-target 1 '' \
    'command display-panes: -t argument must be a string'
expect_key typed-target-preserved F12 'F12=display-message -p preserved'

main_client set-option -s 'command-alias[90]' \
    'zzpanes10i=display-panes -b'
main_client set-option -s 'command-alias[91]' \
    'zzdisplay10i=display-message -p'

bindings="$work/bindings.conf"
printf '%s\n' \
    'bind-key -T zzpanes10i F1 display-panes { display -p typed }' \
    'bind-key -T zzpanes10i F2 display-panes "{ display -p quoted }"' \
    'bind-key -T zzpanes10i F3 display-panes -bN { display -p flags }' \
    'bind-key -T zzpanes10i F4 display-panes -- { display -p boundary }' \
    'bind-key -T zzpanes10i F5 displayp { display -p builtin-alias }' \
    'bind-key -T zzpanes10i F6 display-pa { display -p builtin-prefix }' \
    'bind-key -T zzpanes10i F7 zzpanes10i { display -p outer-user }' \
    'bind-key -T zzpanes10i F8 display-panes { display -p child-alias }' \
    'bind-key -T zzpanes10i F9 display-panes { display-mes -p child-prefix }' \
    'bind-key -T zzpanes10i F10 display-panes { zzdisplay10i child-user }' \
    'bind-key -T zzpanes10i F11 display-panes { display -p one ; list-s }' \
    >"$bindings"
probe bindings 0 '' ''
expect_key typed-template F1 \
    'F1=display-panes { display-message -p typed }'
expect_key quoted-template F2 \
    'F2=display-panes "{ display -p quoted }"'
expect_key valueless-flags F3 \
    'F3=display-panes -Nb { display-message -p flags }'
expect_key boundary F4 \
    'F4=display-panes { display-message -p boundary }'
expect_key builtin-alias F5 \
    'F5=display-panes { display-message -p builtin-alias }'
expect_key builtin-prefix F6 \
    'F6=display-panes { display-message -p builtin-prefix }'
expect_key user-alias F7 \
    'F7=display-panes -b { display-message -p outer-user }'
expect_key child-alias F8 \
    'F8=display-panes { display-message -p child-alias }'
expect_key child-prefix F9 \
    'F9=display-panes { display-message -p child-prefix }'
expect_key child-user F10 \
    'F10=display-panes { display-message -p child-user }'
expect_key multiple-children F11 \
    'F11=display-panes { display-message -p one ; list-sessions }'

child_before_option="$work/child-before-option.conf"
printf '%s\n' \
    'bind-key -T zzpanes10i F12 display-panes -d { wibble }' \
    >"$child_before_option"
probe child-before-option 1 \
    "$child_before_option:1: unknown command: wibble" ''

child_before_arity="$work/child-before-arity.conf"
printf '%s\n' \
    'bind-key -T zzpanes10i F12 display-panes { wibble } extra' \
    >"$child_before_arity"
probe child-before-arity 1 \
    "$child_before_arity:1: unknown command: wibble" ''

arity="$work/arity.conf"
printf '%s\n' \
    'bind-key -T zzpanes10i F12 display-panes { display-message -p valid } extra' \
    >"$arity"
probe arity 1 '' \
    'command display-panes: too many arguments (need at most 1)'
expect_key arity-preserved F12 'F12=display-message -p preserved'

runtime_probe invalid-duration 1 'no current client' \
    display-panes -d not-a-delay
runtime_probe valid-duration 1 'no current client' \
    display-panes -d 0

if [ "$check_count" -ne 22 ]; then
    fail_check "check-count-$check_count"
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g ARGS_PARSE_DISPLAY_PANES clean:22
else
    failure_labels="$(paste -sd, "$work/failures")"
    failure_side="${ZZ_SMOKE_CANARY:-missing-canary}"
    main_client set-environment -g ARGS_PARSE_DISPLAY_PANES \
        "failed:$failure_side:$failure_labels"
    printf '%s:%s\n' "$failure_side" "$failure_labels" >&2
fi
