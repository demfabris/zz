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

work="$HOME/args-parse-bind-key-work"
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

expect_binding() {
    label="$1"
    table="$2"
    key="$3"
    expected="$4"
    if [ "$(main_client list-keys -T "$table" -F '#{key_string}=#{key_command}' "$key")" != \
        "$expected" ]; then
        fail_check "$label"
    fi
}

expect_binding_metadata() {
    label="$1"
    table="$2"
    key="$3"
    expected="$4"
    if [ "$(main_client list-keys -T "$table" \
        -F '#{key_string}=#{key_note}|#{key_repeat}|#{key_command}' "$key")" != \
        "$expected" ]; then
        fail_check "$label"
    fi
}

expect_absent_environment() {
    label="$1"
    name="$2"
    if main_client show-environment -g "$name" >/dev/null 2>&1; then
        fail_check "$label"
    fi
}

check_count=$((check_count + 1))
main_client set-environment -gu BIND_TYPED_KEY_FORBIDDEN
typed_key="$work/typed-key.conf"
printf '%s\n' \
    'bind-key -T zzbind { set-environment -g BIND_TYPED_KEY_FORBIDDEN yes } display-message -p forbidden' \
    >"$typed_key"
probe typed-key 1 '' \
    'unknown key: set-environment -g BIND_TYPED_KEY_FORBIDDEN yes'
expect_absent_environment typed-key-effect BIND_TYPED_KEY_FORBIDDEN
typed_key_unknown="$work/typed-key-unknown.conf"
printf '%s\n' 'bind-key { wibble } display-message' >"$typed_key_unknown"
probe typed-key-unknown 1 \
    "$typed_key_unknown:1: unknown command: wibble" ''

check_count=$((check_count + 1))
typed_table="$work/typed-table.conf"
printf '%s\n' \
    'bind-key -T { display-message -p typed-table } F1 display-message -p forbidden' \
    >"$typed_table"
probe typed-table 1 \
    "$typed_table:1: command bind-key: -T argument must be a string" ''

check_count=$((check_count + 1))
typed_note="$work/typed-note.conf"
printf '%s\n' \
    'bind-key -T zzbind -N { display-message -p typed-note } F1 display-message -p forbidden' \
    >"$typed_note"
probe typed-note 1 \
    "$typed_note:1: command bind-key: -N argument must be a string" ''

check_count=$((check_count + 1))
typed_single="$work/typed-single.conf"
printf '%s\n' \
    'bind-key -T zzbind -N typed-note F1 { display-message -p typed-single }' \
    >"$typed_single"
probe typed-single 0 '' ''
typed_single_readback="$(main_client list-keys -T zzbind \
    -F '#{key_string}=#{key_note}|#{key_command}' F1)"
if [ "$typed_single_readback" != \
    'F1=typed-note|display-message -p typed-single' ]; then
    fail_check typed-single-readback
fi

check_count=$((check_count + 1))
string_single="$work/string-single.conf"
printf '%s\n' \
    "bind-key -T zzbind F2 'display-message -p string-single'" \
    >"$string_single"
probe string-single 0 '' ''
expect_binding string-single-readback zzbind F2 \
    'F2=display-message -p string-single'

check_count=$((check_count + 1))
variadic_string="$work/variadic-string.conf"
printf '%s\n' \
    'bind-key -T zzbind F3 display-message -p variadic-string' \
    >"$variadic_string"
probe variadic-string 0 '' ''
expect_binding variadic-string-readback zzbind F3 \
    'F3=display-message -p variadic-string'

check_count=$((check_count + 1))
variadic_typed="$work/variadic-typed.conf"
printf '%s\n' \
    'bind-key -T zzbind F4 if-shell -F 1 { display-message -p typed-true } { display-message -p typed-false }' \
    >"$variadic_typed"
probe variadic-typed 0 '' ''
expect_binding variadic-typed-readback zzbind F4 \
    'F4=if-shell -F 1 { display-message -p typed-true } { display-message -p typed-false }'

check_count=$((check_count + 1))
typed_first_extra="$work/typed-first-extra.conf"
printf '%s\n' \
    'bind-key -T zzbind F5 { display-message -p forbidden } trailing' \
    >"$typed_first_extra"
probe typed-first-extra 0 '' ''
expect_binding typed-first-extra-readback zzbind F5 'F5='

check_count=$((check_count + 1))
main_client bind-key -T zzboundary F7 display-message -p preserved
boundaries="$work/boundaries.conf"
printf '%s\n' \
    'bind-key -T zzboundary -- F6 { display-message -p boundary }' \
    'bind-key -T zzboundary F7 -r { display-message -p forbidden }' \
    >"$boundaries"
probe boundaries 1 '' 'unknown command: -r'
expect_binding double-dash-boundary zzboundary F6 \
    'F6=display-message -p boundary'
expect_binding late-flag-preserved zzboundary F7 \
    'F7=display-message -p preserved'

check_count=$((check_count + 1))
main_client set-option -s 'command-alias[90]' 'zzbind84=bind-key -T zzalias'
main_client set-option -s 'command-alias[91]' 'zzdisplay84=display-message -p'
aliases="$work/aliases.conf"
printf '%s\n' \
    'bind -T zzalias F1 { display -p outer-builtin }' \
    'bind-k -T zzalias F2 { display-mes -p outer-prefix }' \
    'zzbind84 F3 { display -p outer-user }' \
    'bind-key -T zzalias F4 { zzdisplay84 inner-user }' \
    >"$aliases"
probe aliases 0 '' ''
alias_readback="$(main_client list-keys -T zzalias \
    -F '#{key_string}=#{key_command}')"
if [ "$alias_readback" != 'F1=display-message -p outer-builtin
F2=display-message -p outer-prefix
F3=display-message -p outer-user
F4=display-message -p inner-user' ]; then
    fail_check aliases-readback
fi

check_count=$((check_count + 1))
empty="$work/empty.conf"
printf '%s\n' \
    'bind-key -T zzempty F8 { }' \
    'display-message -p empty-health' \
    >"$empty"
probe empty 0 empty-health ''
expect_binding empty-readback zzempty F8 'F8='

check_count=$((check_count + 1))
nested="$work/nested.conf"
printf '%s\n' \
    'bind-key -T zznested F9 { if-shell -F 1 { display-message hi } { display-message bye } }' \
    >"$nested"
probe nested 0 '' ''
expect_binding nested-readback zznested F9 \
    'F9=if-shell -F 1 { display-message hi } { display-message bye }'

check_count=$((check_count + 1))
multiline="$work/multiline.conf"
printf '%s\n' \
    'bind-key -T zzmulti F1 {' \
    '  display-message -p typed-first' \
    '  display-message -p typed-second' \
    '}' \
    "bind-key -T zzmulti F2 'display-message -p string-first" \
    "display-message -p string-second'" \
    >"$multiline"
probe multiline 0 '' ''
multiline_readback="$(main_client list-keys -T zzmulti \
    -F '#{key_string}=#{key_command}')"
if [ "$multiline_readback" != 'F1=display-message -p typed-first \; display-message -p typed-second
F2=display-message -p string-first \; display-message -p string-second' ]; then
    fail_check multiline-readback
fi

check_count=$((check_count + 1))
main_client bind-key -T zzstored F10 display-message -p preserved
invalid_replacement="$work/invalid-replacement.conf"
printf '%s\n' \
    'bind-key -T zzstored F10 if-shell -F { display-message -p condition } { display-message -p forbidden }' \
    >"$invalid_replacement"
probe invalid-replacement 1 '' \
    'command if-shell: argument 1 must be "string"'
expect_binding invalid-replacement-preserved zzstored F10 \
    'F10=display-message -p preserved'

check_count=$((check_count + 1))
main_client bind-key -T zzmetadata -N original F6 display-message -p preserved
metadata_bare="$work/metadata-bare.conf"
printf '%s\n' 'bind-key -T zzmetadata F6' >"$metadata_bare"
probe metadata-bare 0 '' ''
expect_binding_metadata metadata-bare-preserves zzmetadata F6 \
    'F6=original|0|display-message -p preserved'
metadata_update="$work/metadata-update.conf"
printf '%s\n' \
    'bind-key -T zzmetadata -N replacement -r F6' \
    'bind-key -T zzmetadata F6' \
    'bind-key -T zzmetadata-empty -r F7' \
    >"$metadata_update"
probe metadata-update 0 '' ''
expect_binding_metadata metadata-update-preserves zzmetadata F6 \
    'F6=replacement|1|display-message -p preserved'
metadata_empty_output="$work/metadata-empty.out"
metadata_empty_error="$work/metadata-empty.err"
set +e
main_client list-keys -T zzmetadata-empty \
    -F '#{key_string}=#{key_command}' >"$metadata_empty_output" 2>"$metadata_empty_error"
metadata_empty_status=$?
set -e
if [ "$metadata_empty_status" -ne 0 ] || [ -s "$metadata_empty_output" ] ||
    [ -s "$metadata_empty_error" ]; then
    fail_check metadata-empty-table
fi
main_client bind-key -T zzmetadata F6 display-message -p replaced
expect_binding_metadata metadata-command-replacement zzmetadata F6 \
    'F6=|0|display-message -p replaced'

check_count=$((check_count + 1))
main_client set-environment -gu BIND_CONTROL_REJECT_FORBIDDEN
main_client set-environment -gu BIND_CONTROL_ACCEPT_FORBIDDEN
control_reject_raw="$work/control-reject.raw"
control_reject_error="$work/control-reject.err"
{
    printf '%s\n' \
        'bind-key -T { set-environment -g BIND_CONTROL_REJECT_FORBIDDEN yes } F11 display-message -p forbidden'
    printf '%s\n' 'detach-client'
} | control_client >"$control_reject_raw" 2>"$control_reject_error" || true
control_accept_raw="$work/control-accept.raw"
control_accept_error="$work/control-accept.err"
set +e
{
    printf '%s\n' \
        'bind -T zzcontrol F11 { set-environment -g BIND_CONTROL_ACCEPT_FORBIDDEN yes }'
    printf '%s\n' 'detach-client'
} | control_client >"$control_accept_raw" 2>"$control_accept_error"
control_accept_status=$?
set -e
if [ "$control_accept_status" -ne 0 ] || [ -s "$control_reject_error" ] ||
    [ -s "$control_accept_error" ] ||
    ! grep -Fq \
        'parse error: command bind-key: -T argument must be a string' \
        "$control_reject_raw" ||
    ! grep -Eq '^%error [0-9]+ [0-9]+ 1$' "$control_reject_raw"; then
    fail_check control-typed-arguments
fi
expect_binding control-accepted-readback zzcontrol F11 \
    'F11=set-environment -g BIND_CONTROL_ACCEPT_FORBIDDEN yes'
expect_absent_environment control-reject-effect BIND_CONTROL_REJECT_FORBIDDEN
expect_absent_environment control-accept-effect BIND_CONTROL_ACCEPT_FORBIDDEN

check_count=$((check_count + 1))
main_client set-environment -gu BIND_DISPATCH_TYPED
main_client set-environment -gu BIND_DISPATCH_STRING
main_client set-environment -gu BIND_DISPATCH_DONE
dispatch="$work/dispatch.conf"
printf '%s\n' \
    'bind-key -T zzdispatch F11 {' \
    '  kill-pane -t missing' \
    '  set-environment -g BIND_DISPATCH_TYPED yes' \
    '}' \
    "bind-key -T zzdispatch F12 'kill-pane -t missing" \
    "set-environment -g BIND_DISPATCH_STRING yes'" \
    'bind-key -T zzdispatch F3 set-environment -g BIND_DISPATCH_DONE yes' \
    'bind-key -T root F3 set-environment -g BIND_DISPATCH_DONE yes' \
    >"$dispatch"
probe dispatch 0 '' ''
outer_tmux="${ZZ_SMOKE_TMUX_BIN:-$(command -v tmux)}"
outer_label="zzbind-input-$$"
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
    fail_check dispatch-client-start
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
    fail_check dispatch-client-attach
elif ! main_client switch-client -c "$target_client" -T zzdispatch ||
    ! outer_client send-keys -t input:0.0 F11; then
    fail_check dispatch-typed-input
fi
attempt=0
while [ "$attempt" -lt 100 ] &&
    [ "$(main_client show-environment -g BIND_DISPATCH_TYPED 2>/dev/null || true)" != \
        'BIND_DISPATCH_TYPED=yes' ]; do
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ "$(main_client show-environment -g BIND_DISPATCH_TYPED 2>/dev/null || true)" != \
    'BIND_DISPATCH_TYPED=yes' ]; then
    fail_check dispatch-typed-group
fi
if [ -n "$target_client" ]; then
    if ! main_client switch-client -c "$target_client" -T zzdispatch ||
        ! outer_client send-keys -t input:0.0 F12 F3; then
        fail_check dispatch-string-input
    fi
    attempt=0
    while [ "$attempt" -lt 100 ] &&
        [ "$(main_client show-environment -g BIND_DISPATCH_DONE 2>/dev/null || true)" != \
            'BIND_DISPATCH_DONE=yes' ]; do
        attempt=$((attempt + 1))
        sleep 0.05
    done
    if [ "$(main_client show-environment -g BIND_DISPATCH_DONE 2>/dev/null || true)" != \
        'BIND_DISPATCH_DONE=yes' ]; then
        fail_check dispatch-string-input-pending
    fi
fi
expect_absent_environment dispatch-string-group BIND_DISPATCH_STRING
cleanup_outer
main_client resize-window -t w -x 80 -y 23

if [ "$failed" -eq 0 ] && [ "$check_count" -eq 17 ]; then
    main_client set-environment -g ARGS_PARSE_BIND_KEY clean:17
elif [ -s "$work/failures" ]; then
    failure_labels="$(paste -sd, "$work/failures")"
    failure_side="${ZZ_SMOKE_CANARY:-missing-canary}"
    main_client set-environment -g ARGS_PARSE_BIND_KEY \
        "failed:$failure_side:$failure_labels"
    printf '%s:%s\n' "$failure_side" "$failure_labels" >&2
fi
