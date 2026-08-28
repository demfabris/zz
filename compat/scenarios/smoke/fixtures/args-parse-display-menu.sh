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
    initial_control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" -C "$@"
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
    initial_control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" -C "$@"
    }
fi

work="$HOME/args-parse-display-menu-work"
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
    actual="$(main_client list-keys -T zzmenu10h \
        -F '#{key_string}=#{key_command}' "$key")"
    status=$?
    set -e
    if [ "$status" -ne 0 ] || [ "$actual" != "$expected" ]; then
        fail_check "$label"
    fi
}

control_probe() {
    label="$1"
    expected_status="$2"
    expected_terminator="$3"
    expected_payload="$4"
    command="$5"
    raw="$work/control-$label.raw"
    error="$work/control-$label.err"
    check_count=$((check_count + 1))
    set +e
    printf '%s\n' "$command" | control_client >"$raw" 2>"$error"
    status=$?
    set -e
    if [ "$status" -ne "$expected_status" ] || [ -s "$error" ] ||
        ! awk -v terminator="$expected_terminator" \
            -v payload="$expected_payload" '
            NR == 1 {
                if ($1 != "%begin" || $2 !~ /^[0-9]+$/ ||
                    $3 !~ /^[0-9]+$/ || $4 != 0) bad = 1
                attach_time = $2
                attach_number = $3
                next
            }
            NR == 2 {
                if ($1 != "%end" || $2 != attach_time ||
                    $3 != attach_number || $4 != 0) bad = 1
                next
            }
            NR == 3 {
                if ($0 !~ /^%session-changed \$[0-9]+ w$/) bad = 1
                next
            }
            NR == 4 {
                if ($1 != "%begin" || $2 !~ /^[0-9]+$/ ||
                    $3 !~ /^[0-9]+$/ || $4 != 1) bad = 1
                command_time = $2
                command_number = $3
                next
            }
            payload != "" && NR == 5 {
                if ($0 != payload) bad = 1
                next
            }
            (payload == "" && NR == 5) || (payload != "" && NR == 6) {
                if ($1 != terminator || $2 != command_time ||
                    $3 != command_number || $4 != 1) bad = 1
                next
            }
            (payload == "" && NR == 6) || (payload != "" && NR == 7) {
                if ($0 != "%exit") bad = 1
                next
            }
            { bad = 1 }
            END {
                expected_lines = payload == "" ? 6 : 7
                exit bad || NR != expected_lines
            }
        ' "$raw"; then
        fail_check "control-$label"
    fi
}

incomplete_control_probe() {
    label="$1"
    initial_raw="$work/control-$label-initial.raw"
    initial_error="$work/control-$label-initial.err"
    attached_raw="$work/control-$label-attached.raw"
    attached_error="$work/control-$label-attached.err"
    fifo="$work/control-$label-$$.fifo"
    check_count=$((check_count + 1))
    set +e
    initial_control_client display-menu Incomplete i \
        >"$initial_raw" 2>"$initial_error"
    initial_status=$?
    set -e
    invalid=0
    if [ "$initial_status" -ne 1 ] || [ -s "$initial_error" ]; then
        invalid=1
    fi
    if ! awk '
            NR == 1 {
                if ($1 != "%begin" || $2 !~ /^[0-9]+$/ ||
                    $3 !~ /^[0-9]+$/ || $4 != 0) bad = 1
                command_time = $2
                command_number = $3
                next
            }
            NR == 2 {
                if ($0 != "no current client") bad = 1
                next
            }
            NR == 3 {
                if ($1 != "%error" || $2 != command_time ||
                    $3 != command_number || $4 != 0) bad = 1
                next
            }
            NR == 4 {
                if ($0 != "%exit") bad = 1
                next
            }
            { bad = 1 }
            END { exit bad || NR != 4 }
        ' "$initial_raw"; then
        invalid=1
    fi

    mkfifo "$fifo"
    control_client <"$fifo" >"$attached_raw" 2>"$attached_error" &
    control_pid=$!
    exec 3>"$fifo"
    printf '%s\n' 'display-menu Incomplete i' >&3
    frame_seen=0
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if grep -Eq '^%error [0-9]+ [0-9]+ 1$' \
            "$attached_raw" 2>/dev/null; then
            frame_seen=1
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.01
    done
    exec 3>&-
    set +e
    wait "$control_pid"
    attached_status=$?
    set -e
    if [ "$frame_seen" -ne 1 ] || [ "$attached_status" -ne 1 ] ||
        [ -s "$attached_error" ]; then
        invalid=1
    fi
    if ! awk '
            NR == 1 {
                if ($1 != "%begin" || $2 !~ /^[0-9]+$/ ||
                    $3 !~ /^[0-9]+$/ || $4 != 0) bad = 1
                attach_time = $2
                attach_number = $3
                next
            }
            NR == 2 {
                if ($1 != "%end" || $2 != attach_time ||
                    $3 != attach_number || $4 != 0) bad = 1
                next
            }
            NR == 3 {
                if ($0 !~ /^%session-changed \$[0-9]+ w$/) bad = 1
                next
            }
            NR == 4 {
                if ($1 != "%begin" || $2 !~ /^[0-9]+$/ ||
                    $3 !~ /^[0-9]+$/ || $4 != 1) bad = 1
                command_time = $2
                command_number = $3
                next
            }
            NR == 5 {
                if ($0 != "not enough arguments") bad = 1
                next
            }
            NR == 6 {
                if ($1 != "%error" || $2 != command_time ||
                    $3 != command_number || $4 != 1) bad = 1
                next
            }
            NR == 7 {
                if ($0 != "%exit") bad = 1
                next
            }
            { bad = 1 }
            END { exit bad || NR != 7 }
        ' "$attached_raw"; then
        invalid=1
    fi
    if [ "$invalid" -ne 0 ]; then
        fail_check "control-$label"
    fi
}

main_client set-option -s 'command-alias[90]' 'zzmenu10h=display-menu'
main_client set-option -s 'command-alias[91]' \
    'zzchild10h=display-message -p'

typed_name="$work/typed-name.conf"
printf '%s\n' \
    'display-menu { display-message -p name } n { display-message -p action }' \
    >"$typed_name"
probe typed-name 1 \
    "$typed_name:1: command display-menu: argument 1 must be \"string\"" ''

typed_key="$work/typed-key.conf"
printf '%s\n' \
    'display-menu Name { display-message -p key } { display-message -p action }' \
    >"$typed_key"
probe typed-key 1 \
    "$typed_key:1: command display-menu: argument 2 must be \"string\"" ''

typed_later_name="$work/typed-later-name.conf"
printf '%s\n' \
    'display-menu One o { display-message -p one } { display-message -p name } n { display-message -p two }' \
    >"$typed_later_name"
probe typed-later-name 1 \
    "$typed_later_name:1: command display-menu: argument 4 must be \"string\"" ''

typed_later_key="$work/typed-later-key.conf"
printf '%s\n' \
    'display-menu One o { display-message -p one } Two { display-message -p key } { display-message -p two }' \
    >"$typed_later_key"
probe typed-later-key 1 \
    "$typed_later_key:1: command display-menu: argument 5 must be \"string\"" ''

typed_shifted_name="$work/typed-shifted-name.conf"
printf '%s\n' \
    "display-menu '' { display-message -p name } n { display-message -p action }" \
    >"$typed_shifted_name"
probe typed-shifted-name 1 \
    "$typed_shifted_name:1: command display-menu: argument 2 must be \"string\"" ''

child_precedence="$work/child-precedence.conf"
printf '%s\n' \
    'display-menu { no-such-display-menu-child } n { display-message -p action }' \
    >"$child_precedence"
probe child-precedence 1 \
    "$child_precedence:1: unknown command: no-such-display-menu-child" ''

for option_spec in \
    'b:option-lower-b' \
    'c:option-lower-c' \
    'C:option-upper-C' \
    'H:option-upper-H' \
    's:option-lower-s' \
    'S:option-upper-S' \
    't:option-lower-t' \
    'T:option-upper-T' \
    'x:option-lower-x' \
    'y:option-lower-y'
do
    option=${option_spec%%:*}
    label=${option_spec#*:}
    config="$work/$label.conf"
    printf 'display-menu -%s { display-message -p option } One o { display-message -p action }\n' \
        "$option" >"$config"
    probe "$label" 1 \
        "$config:1: command display-menu: -$option argument must be a string" ''
done

bindings="$work/bindings.conf"
printf '%s\n' \
    'bind-key -T zzmenu10h F1 display-menu One o { display -p typed }' \
    'bind-key -T zzmenu10h F2 menu One o { display-mes -p prefix-child }' \
    'bind-key -T zzmenu10h F3 display-men One o { zzchild10h user-child }' \
    'bind-key -T zzmenu10h F4 zzmenu10h One o { zzchild10h outer-user }' \
    "bind-key -T zzmenu10h F5 display-menu '' Two t { display -p shifted }" \
    'bind-key -T zzmenu10h F6 display-menu One o "{ display -p quoted }"' \
    'bind-key -T zzmenu10h F7 display-menu One o { display -p one } Two t { list-s }' \
    "bind-key -T zzmenu10h F8 display-menu '' '' Three t { display -p three }" \
    'bind-key -T zzmenu10h F9 display-menu Incomplete' \
    'bind-key -T zzmenu10h F10 display-menu Incomplete i' \
    >"$bindings"
probe bindings 0 '' ''
expect_key canonical F1 \
    'F1=display-menu One o { display-message -p typed }'
expect_key builtin-alias F2 \
    'F2=display-menu One o { display-message -p prefix-child }'
expect_key builtin-prefix F3 \
    'F3=display-menu One o { display-message -p user-child }'
expect_key user-aliases F4 \
    'F4=display-menu One o { display-message -p outer-user }'
expect_key separator-shifted-action F5 \
    "F5=display-menu '' Two t { display-message -p shifted }"
expect_key quoted-action F6 \
    'F6=display-menu One o "{ display -p quoted }"'
expect_key multiple-items F7 \
    'F7=display-menu One o { display-message -p one } Two t { list-sessions }'
expect_key multiple-separators F8 \
    "F8=display-menu '' '' Three t { display-message -p three }"
expect_key incomplete-name-constructed F9 \
    'F9=display-menu Incomplete'
expect_key incomplete-key-constructed F10 \
    'F10=display-menu Incomplete i'

main_client bind-key -T zzmenu10h F11 display-message -p preserved
preserve="$work/preserve.conf"
printf '%s\n' \
    'bind-key -T zzmenu10h F11 display-menu { display-message -p name } n { display-message -p action }' \
    >"$preserve"
probe preserve 1 '' \
    'command display-menu: argument 1 must be "string"'
expect_key preserved-binding F11 'F11=display-message -p preserved'

incomplete_name="$work/incomplete-name.conf"
printf '%s\n' 'display-menu Incomplete' >"$incomplete_name"
probe incomplete-name 1 '' 'no current client'

incomplete_key="$work/incomplete-key.conf"
printf '%s\n' 'display-menu Incomplete i' >"$incomplete_key"
probe incomplete-key 1 '' 'no current client'

control_probe valid 0 '%end' '' \
    'display-menu One o { display-message -p action }'
control_probe typed-name 0 '%error' \
    'parse error: command display-menu: argument 1 must be "string"' \
    'display-menu { display-message -p name } n { display-message -p action }'
incomplete_control_probe incomplete

if [ "$check_count" -ne 34 ]; then
    fail_check "check-count-$check_count"
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g ARGS_PARSE_DISPLAY_MENU clean:34
else
    failure_labels="$(paste -sd, "$work/failures")"
    failure_side="${ZZ_SMOKE_CANARY:-missing-canary}"
    main_client set-environment -g ARGS_PARSE_DISPLAY_MENU \
        "failed:$failure_side:$failure_labels"
    printf '%s:%s\n' "$failure_side" "$failure_labels" >&2
fi
