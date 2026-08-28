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

work="$HOME/args-parse-confirm-before-work"
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

expect_keys() {
    label="$1"
    table="$2"
    expected="$3"
    set +e
    actual="$(main_client list-keys -T "$table" \
        -F '#{key_string}=#{key_command}')"
    status=$?
    set -e
    if [ "$status" -ne 0 ] || [ "$actual" != "$expected" ]; then
        fail_check "$label"
    fi
}

expect_key() {
    label="$1"
    table="$2"
    key="$3"
    expected="$4"
    set +e
    actual="$(main_client list-keys -T "$table" \
        -F '#{key_string}=#{key_command}' "$key")"
    status=$?
    set -e
    if [ "$status" -ne 0 ] || [ "$actual" != "$expected" ]; then
        fail_check "$label"
    fi
}

control_probe() {
    label="$1"
    expected="$2"
    command="$3"
    raw="$work/control-$label.raw"
    error="$work/control-$label.err"
    check_count=$((check_count + 1))
    set +e
    {
        printf '%s\n' "$command"
        printf '%s\n' 'detach-client'
    } | control_client >"$raw" 2>"$error"
    status=$?
    set -e
    if [ "$status" -ne 0 ] || [ -s "$error" ] ||
        ! awk -v expected="$expected" '
            function finish_frame(kind, flags) {
                if (!active) {
                    bad = 1
                    return
                }
                if (kind == "error") {
                    errors++
                    if (flags != 1 || payload != expected) bad = 1
                } else if (payload != "") {
                    bad = 1
                }
                active = 0
                payload = ""
            }
            /^%begin [0-9]+ [0-9]+ [0-9]+$/ {
                if (active) bad = 1
                active = 1
                payload = ""
                next
            }
            /^%end [0-9]+ [0-9]+ [0-9]+$/ {
                finish_frame("end", $4)
                next
            }
            /^%error [0-9]+ [0-9]+ [0-9]+$/ {
                finish_frame("error", $4)
                next
            }
            /^%session-changed \$[0-9]+ w$/ {
                if (active) bad = 1
                next
            }
            /^%exit$/ {
                if (active) bad = 1
                next
            }
            /^%/ {
                bad = 1
                next
            }
            {
                if (!active) {
                    if ($0 != "") bad = 1
                    next
                }
                if (payload != "") payload = payload "\n"
                payload = payload $0
            }
            END { exit bad || active || errors != 1 }
        ' "$raw"; then
        fail_check "control-$label"
    fi
}

for alias in \
    'command-alias[90]=zzconfirm84=confirm-before -c x' \
    'command-alias[91]=inner10e=display-message -p nested' \
    'command-alias[92]=outer10e=confirm-before { inner10e }' \
    'command-alias[93]=loop10e=confirm-before { loop10e }'
do
    name=${alias%%=*}
    value=${alias#*=}
    if ! main_client set-option -s "$name" "$value"; then
        fail_check alias-setup
    fi
done

bindings="$work/bindings.conf"
printf '%s\n' \
    'bind-key -T zzconfirm F1 confirm-before { display -p typed }' \
    "bind-key -T zzconfirm F2 confirm-before 'display -p string'" \
    'bind-key -T zzconfirm F3 confirm-before "{ display -p quoted }"' \
    'bind -T zzconfirm F4 confirm { display -p builtin-alias }' \
    'bind-k -T zzconfirm F5 confirm-b { display -p builtin-prefix }' \
    'bind-key -T zzconfirm F6 zzconfirm84 { display -p user-alias }' \
    'bind-key -T zzconfirm F7 confirm-before -c x -- { display -p boundary }' \
    'bind-key -T zzconfirm F8 confirm-before -p ok {}' \
    'bind-key -T zzconfirm F9 confirm-before { if-shell -F 1 { inner10e } }' \
    'bind-key -T zzconfirm F10 confirm-before {' \
    '  display -p physical-one' \
    '  display -p physical-two' \
    '}' \
    >"$bindings"
probe bindings 0 '' ''
expect_keys bindings-readback zzconfirm \
    'F1=confirm-before { display-message -p typed }
F2=confirm-before "display -p string"
F3=confirm-before "{ display -p quoted }"
F4=confirm-before { display-message -p builtin-alias }
F5=confirm-before { display-message -p builtin-prefix }
F6=confirm-before -c x { display-message -p user-alias }
F7=confirm-before -c x { display-message -p boundary }
F8=confirm-before -p ok {  }
F9=confirm-before { if-shell -F 1 { display-message -p nested } }
F10=confirm-before { display-message -p physical-one ;; display-message -p physical-two }'

typed_c="$work/typed-c.conf"
printf '%s\n' \
    'confirm-before -c { display-message -p key } { display-message -p body }' \
    >"$typed_c"
probe typed-c 1 \
    "$typed_c:1: command confirm-before: -c argument must be a string" ''

typed_p="$work/typed-p.conf"
printf '%s\n' \
    'confirm-before -p { display-message -p prompt } { display-message -p body }' \
    >"$typed_p"
probe typed-p 1 \
    "$typed_p:1: command confirm-before: -p argument must be a string" ''

typed_t="$work/typed-t.conf"
printf '%s\n' \
    'confirm-before -t { display-message -p target } { display-message -p body }' \
    >"$typed_t"
probe typed-t 1 \
    "$typed_t:1: command confirm-before: -t argument must be a string" ''

typed_unknown="$work/typed-unknown.conf"
printf '%s\n' 'confirm-before { wibble }' >"$typed_unknown"
probe typed-unknown 1 "$typed_unknown:1: unknown command: wibble" ''

recursive_alias="$work/recursive-alias.conf"
printf '%s\n' 'confirm-before { loop10e }' >"$recursive_alias"
probe recursive-alias 1 \
    "$recursive_alias:1: unknown command: loop10e" ''

outer_alias="$work/outer-alias.conf"
printf '%s\n' 'outer10e' >"$outer_alias"
probe outer-alias 1 "$outer_alias:1: unknown command: inner10e" ''

stored="$work/stored.conf"
printf '%s\n' \
    'bind-key -T zzstored F10 confirm-before { display-message -p preserved }' \
    'bind-key -T zzstored F10 confirm-before { display-message -p forbidden } -y' \
    >"$stored"
probe stored 1 '' \
    'command confirm-before: too many arguments (need at most 1)'
expect_key stored-preserved zzstored F10 \
    'F10=confirm-before { display-message -p preserved }'

check_count=$((check_count + 1))
control_accept_raw="$work/control-accept.raw"
control_accept_error="$work/control-accept.err"
set +e
{
    printf '%s\n' \
        'bind-key -T zzcontrol F8 confirm-b { display -p control-prefix }'
    printf '%s\n' 'detach-client'
} | control_client >"$control_accept_raw" 2>"$control_accept_error"
control_accept_status=$?
set -e
if [ "$control_accept_status" -ne 0 ] || [ -s "$control_accept_error" ] ||
    grep -Eq '^%error ' "$control_accept_raw"; then
    fail_check control-accept
fi
expect_key control-readback zzcontrol F8 \
    'F8=confirm-before { display-message -p control-prefix }'

control_probe typed-c \
    'parse error: command confirm-before: -c argument must be a string' \
    'confirm-before -c { display-message -p key } { display-message -p body }'
control_probe typed-p \
    'parse error: command confirm-before: -p argument must be a string' \
    'confirm-before -p { display-message -p prompt } { display-message -p body }'
control_probe typed-t \
    'parse error: command confirm-before: -t argument must be a string' \
    'confirm-before -t { display-message -p target } { display-message -p body }'
control_probe typed-unknown 'parse error: unknown command: wibble' \
    'confirm-before { wibble }'
control_probe nested-unknown 'parse error: unknown command: wibble' \
    'bind-key -T zzcontrol F11 confirm-before { wibble }'
control_probe string-unknown 'unknown command: wibble' \
    "confirm-before 'wibble'"
control_probe quoted-brace 'syntax error' \
    'confirm-before -c xx "{ display-message -p quoted }"'
control_probe double-dash 'invalid confirm key' \
    'confirm-before -c xx -- { display-message -p boundary }'

if ! main_client set-option -g @confirm-payload '" ; wibble'; then
    fail_check format-setup
fi
control_probe format-string 'unknown command: wibble' \
    "confirm-before -c xx 'display-message -p \"#{@confirm-payload}\"'"
control_probe format-typed 'invalid confirm key' \
    'confirm-before -c xx { display-message -p "#{@confirm-payload}" }'

if [ "$check_count" -ne 19 ]; then
    fail_check "check-count-$check_count"
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g ARGS_PARSE_CONFIRM_BEFORE clean:19
else
    failure_labels="$(paste -sd, "$work/failures")"
    failure_side="${ZZ_SMOKE_CANARY:-missing-canary}"
    main_client set-environment -g ARGS_PARSE_CONFIRM_BEFORE \
        "failed:$failure_side:$failure_labels"
    printf '%s:%s\n' "$failure_side" "$failure_labels" >&2
fi
