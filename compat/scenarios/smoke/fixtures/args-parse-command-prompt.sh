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

work="$HOME/args-parse-command-prompt-work"
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
    table="$2"
    key="$3"
    expected="$4"
    check_count=$((check_count + 1))
    set +e
    actual="$(main_client list-keys -T "$table" \
        -F '#{key_string}=#{key_command}' "$key")"
    status=$?
    set -e
    if [ "$status" -ne 0 ] || [ "$actual" != "$expected" ]; then
        fail_check "$label"
    fi
}

expect_environment() {
    label="$1"
    name="$2"
    expected_value="$3"
    check_count=$((check_count + 1))
    set +e
    actual="$(main_client show-environment -g "$name" 2>/dev/null)"
    status=$?
    set -e
    if [ "$status" -ne 0 ] || [ "$actual" != "$name=$expected_value" ]; then
        fail_check "$label"
    fi
}

expect_absent_environment() {
    label="$1"
    name="$2"
    check_count=$((check_count + 1))
    if main_client show-environment -g "$name" >/dev/null 2>&1; then
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
    'command-alias[94]=cpalias10f=set-environment -g CP_ALIAS_FROZEN' \
    'command-alias[95]=cpchild10f=set-environment -g CP_ALIAS_SIBLING' \
    'command-alias[96]=cpouter10f=command-prompt { cpchild10f yes }'
do
    name=${alias%%=*}
    value=${alias#*=}
    if ! main_client set-option -s "$name" "$value"; then
        fail_check alias-setup
    fi
done

bindings="$work/bindings.conf"
printf '%s\n' \
    'bind-key -T root F1 command-prompt' \
    'bind-key -T root F2 command-prompt { cpalias10f typed }' \
    "bind-key -T root F3 command-prompt 'cpalias10f string'" \
    "bind-key -T root F4 command-prompt { set-environment -g CP_TYPED_SUB '%%:%%:%1:%1' }" \
    "bind-key -T root F5 command-prompt \"set-environment -g CP_STRING_SUB '%%:%%:%1:%1'\"" \
    "bind-key -T root F6 command-prompt 'set-environment -g CP_TRIPLE %%% ; set-environment -g CP_INDEX %1%'" \
    'bind-key -T root F7 command-prompt { set-environment -g CP_TYPED_SAFE %% }' \
    'bind-key -T root F8 command-prompt {' \
    '  kill-pane -t missing ; set-environment -g CP_TYPED_SAME yes' \
    '  set-environment -g CP_TYPED_LATER yes' \
    '}' \
    "bind-key -T root F9 command-prompt 'kill-pane -t missing
set-environment -g CP_STRING_LATER yes'" \
    'bind-key -T root F10 command-prompt {}' \
    'bind-key -T root F12 set-environment -g CP_INPUT_SYNC yes' \
    >"$bindings"
probe bindings 0 '' ''
expect_key zero-positional root F1 'F1=command-prompt'
expect_key typed-positional root F2 \
    'F2=command-prompt { set-environment -g CP_ALIAS_FROZEN typed }'
expect_key string-positional root F3 'F3=command-prompt "cpalias10f string"'
expect_key typed-groups root F8 \
    'F8=command-prompt { kill-pane -t missing ; set-environment -g CP_TYPED_SAME yes ;; set-environment -g CP_TYPED_LATER yes }'
expect_key empty-typed root F10 'F10=command-prompt {  }'

typed_i="$work/typed-I.conf"
printf '%s\n' \
    'command-prompt -I { display-message -p input } { display-message -p body }' \
    >"$typed_i"
probe typed-I 1 \
    "$typed_i:1: command command-prompt: -I argument must be a string" ''

typed_p="$work/typed-p.conf"
printf '%s\n' \
    'command-prompt -p { display-message -p prompt } { display-message -p body }' \
    >"$typed_p"
probe typed-p 1 \
    "$typed_p:1: command command-prompt: -p argument must be a string" ''

typed_t="$work/typed-t.conf"
printf '%s\n' \
    'command-prompt -t { display-message -p target } { display-message -p body }' \
    >"$typed_t"
probe typed-t 1 \
    "$typed_t:1: command command-prompt: -t argument must be a string" ''

typed_T="$work/typed-T.conf"
printf '%s\n' \
    'command-prompt -T { display-message -p type } { display-message -p body }' \
    >"$typed_T"
probe typed-T 1 \
    "$typed_T:1: command command-prompt: -T argument must be a string" ''

too_many="$work/too-many.conf"
printf '%s\n' 'command-prompt one two' >"$too_many"
probe too-many 1 \
    "$too_many:1: command command-prompt: too many arguments (need at most 1)" ''

typed_unknown="$work/typed-unknown.conf"
printf '%s\n' 'command-prompt { unknown10f }' >"$typed_unknown"
probe typed-unknown 1 "$typed_unknown:1: unknown command: unknown10f" ''

child_before_type="$work/child-before-type.conf"
printf '%s\n' \
    'command-prompt -I { child10f } { display-message -p body }' \
    >"$child_before_type"
probe child-before-type 1 \
    "$child_before_type:1: unknown command: child10f" ''

child_before_name="$work/child-before-name.conf"
printf '%s\n' 'parent10f { child10f }' >"$child_before_name"
probe child-before-name 1 \
    "$child_before_name:1: unknown command: child10f" ''

child_before_arity="$work/child-before-arity.conf"
printf '%s\n' 'command-prompt one { child10f }' >"$child_before_arity"
probe child-before-arity 1 \
    "$child_before_arity:1: unknown command: child10f" ''

outer_alias="$work/outer-alias.conf"
printf '%s\n' \
    'bind-key -T zzprompt-alias F1 cpouter10f' \
    'bind-key -T zzprompt-alias F2 command-prompt { cpchild10f yes }' \
    >"$outer_alias"
probe outer-alias 1 '' 'unknown command: cpchild10f'
expect_key alias-sibling zzprompt-alias F2 \
    'F2=command-prompt { set-environment -g CP_ALIAS_SIBLING yes }'

control_probe typed-I \
    'parse error: command command-prompt: -I argument must be a string' \
    'command-prompt -I { display-message -p input } { display-message -p body }'
control_probe too-many \
    'parse error: command command-prompt: too many arguments (need at most 1)' \
    'command-prompt one two'
control_probe typed-unknown 'parse error: unknown command: unknown10f' \
    'command-prompt { unknown10f }'
control_probe child-before-arity 'parse error: unknown command: child10f' \
    'command-prompt one { child10f }'

if ! main_client set-option -s 'command-alias[94]' \
    'cpalias10f=set-environment -g CP_ALIAS_FRESH'; then
    fail_check alias-refresh
fi

for name in \
    CP_ZERO \
    CP_ALIAS_FROZEN \
    CP_ALIAS_FRESH \
    CP_TYPED_SUB \
    CP_STRING_SUB \
    CP_TRIPLE \
    CP_INDEX \
    CP_TYPED_SAFE \
    CP_INJECTED \
    CP_TYPED_SAME \
    CP_TYPED_LATER \
    CP_STRING_LATER \
    CP_INPUT_SYNC
do
    main_client set-environment -gu "$name" >/dev/null 2>&1 || true
done

outer_tmux="${ZZ_SMOKE_TMUX_BIN:-$(command -v tmux)}"
outer_label="zzprompt-input-$$"
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

attach_script="$work/attach-client.sh"
printf '%s\n' \
    '#!/bin/sh' \
    "if [ -n \"\${ZZ_SMOKE_ZZ_BIN:-}\" ]; then" \
    "  exec env -u TMUX -u TMUX_PANE \"\$ZZ_SMOKE_ZZ_BIN\" --socket \"\$ZZ_SMOKE_ZZ_SOCKET\" attach-session -t w" \
    'fi' \
    "exec env -u TMUX -u TMUX_PANE \"\$ZZ_SMOKE_TMUX_BIN\" -L \"\$ZZ_SMOKE_TMUX_LABEL\" attach-session -t w" \
    >"$attach_script"
if ! outer_client -f /dev/null new-session -d -x 80 -y 24 -s input \
    "sh '$attach_script'"; then
    fail_check prompt-client-start
fi
outer_socket="$(outer_client display-message -p '#{socket_path}' 2>/dev/null || true)"
attempt=0
while [ "$attempt" -lt 100 ] && [ -z "$target_client" ]; do
    target_client="$(main_client list-clients -F '#{client_name}' 2>/dev/null |
        head -n 1 || true)"
    attempt=$((attempt + 1))
    [ -n "$target_client" ] || sleep 0.05
done
if [ -z "$target_client" ]; then
    fail_check prompt-client-attach
fi

submit_prompt() {
    label="$1"
    key="$2"
    answer="$3"
    check_count=$((check_count + 1))
    main_client set-environment -gu CP_INPUT_SYNC >/dev/null 2>&1 || true
    if [ -z "$target_client" ] ||
        ! outer_client send-keys -t input:0.0 "$key"; then
        fail_check "$label"
        return
    fi
    sleep 0.1
    if ! outer_client send-keys -t input:0.0 -l "$answer" ||
        ! outer_client send-keys -t input:0.0 Enter; then
        fail_check "$label"
        return
    fi
    sleep 0.1
    if ! outer_client send-keys -t input:0.0 F12; then
        fail_check "$label"
        return
    fi
    attempt=0
    while [ "$attempt" -lt 100 ] &&
        [ "$(main_client show-environment -g CP_INPUT_SYNC 2>/dev/null || true)" != \
            'CP_INPUT_SYNC=yes' ]; do
        attempt=$((attempt + 1))
        sleep 0.05
    done
    if [ "$(main_client show-environment -g CP_INPUT_SYNC 2>/dev/null || true)" != \
        'CP_INPUT_SYNC=yes' ]; then
        fail_check "$label"
    fi
}

submit_prompt submit-zero F1 'set-environment -g CP_ZERO yes'
submit_prompt submit-typed-alias F2 go
submit_prompt submit-string-alias F3 go
submit_prompt submit-typed-substitution F4 abc
submit_prompt submit-string-substitution F5 abc
special='x"\$;~'
submit_prompt submit-escaped-substitution F6 "$special"
injection='x" ; set-environment -g CP_INJECTED yes ; display-message -p "'
submit_prompt submit-typed-safety F7 "$injection"
submit_prompt submit-typed-groups F8 go
submit_prompt submit-string-group F9 go
submit_prompt submit-empty-typed F10 ignored

expect_environment zero-result CP_ZERO yes
expect_environment typed-alias-frozen CP_ALIAS_FROZEN typed
expect_environment string-alias-fresh CP_ALIAS_FRESH string
expect_environment typed-substitution CP_TYPED_SUB 'abc:%%:abc:abc'
expect_environment string-substitution CP_STRING_SUB 'abc:%%:abc:abc'
expect_environment triple-percent CP_TRIPLE "$special"
expect_environment indexed-percent CP_INDEX "$special"
expect_environment typed-special-value CP_TYPED_SAFE "$injection"
expect_absent_environment typed-no-injection CP_INJECTED
expect_absent_environment typed-same-group-stopped CP_TYPED_SAME
expect_environment typed-next-group-ran CP_TYPED_LATER yes
expect_absent_environment string-newlines-one-group CP_STRING_LATER

cleanup_outer
main_client resize-window -t w -x 80 -y 23

if [ "$check_count" -ne 43 ]; then
    fail_check "check-count-$check_count"
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g ARGS_PARSE_COMMAND_PROMPT clean:43
else
    failure_labels="$(paste -sd, "$work/failures")"
    failure_side="${ZZ_SMOKE_CANARY:-missing-canary}"
    main_client set-environment -g ARGS_PARSE_COMMAND_PROMPT \
        "failed:$failure_side:$failure_labels"
    printf '%s:%s\n' "$failure_side" "$failure_labels" >&2
fi
