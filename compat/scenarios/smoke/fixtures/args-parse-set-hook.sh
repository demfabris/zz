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

work="$HOME/args-parse-set-hook-work"
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

expect_absent_environment() {
    label="$1"
    name="$2"
    if main_client show-environment -g "$name" >/dev/null 2>&1; then
        fail_check "$label"
    fi
}

expect_hook() {
    label="$1"
    name="$2"
    expected="$3"
    if [ "$(main_client show-hooks -g "$name")" != "$expected" ]; then
        fail_check "$label"
    fi
}

main_client set-option -s 'command-alias[90]' 'zzhook84=set-hook -g'
main_client set-option -s 'command-alias[91]' 'zzdisplay84=display-message -p'

main_client set-environment -gu SET_HOOK_NAME_FORBIDDEN
typed_name="$work/typed-name.conf"
printf '%s\n' \
    'set-hook -g { set-environment -g SET_HOOK_NAME_FORBIDDEN yes } display-message -p forbidden' \
    >"$typed_name"
probe typed-name 1 \
    "$typed_name:1: command set-hook: argument 1 must be \"string\"" ''
expect_absent_environment typed-name-effect SET_HOOK_NAME_FORBIDDEN

main_client set-environment -gu SET_HOOK_TARGET_FORBIDDEN
typed_target="$work/typed-target.conf"
printf '%s\n' \
    'set-hook -t { set-environment -g SET_HOOK_TARGET_FORBIDDEN yes } after-select-window display-message' \
    >"$typed_target"
probe typed-target 1 \
    "$typed_target:1: command set-hook: -t argument must be a string" ''
expect_absent_environment typed-target-effect SET_HOOK_TARGET_FORBIDDEN

typed_monitor="$work/typed-monitor.conf"
printf '%s\n' \
    'set-hook -B { display-message -p forbidden-monitor } @monitor' \
    >"$typed_monitor"
probe typed-monitor 1 \
    "$typed_monitor:1: command set-hook: -B argument must be a string" ''

main_client set-hook -g after-select-window 'display-message -p preserved-extra'
typed_extra="$work/typed-extra.conf"
printf '%s\n' \
    'set-hook -g after-select-window replacement { display-message -p forbidden-extra }' \
    >"$typed_extra"
probe typed-extra 1 \
    "$typed_extra:1: command set-hook: argument 3 must be \"string\"" ''
expect_hook typed-extra-preserved after-select-window \
    'after-select-window[0] display-message -p preserved-extra'

string_extra="$work/string-extra.conf"
printf '%s\n' \
    'set-hook -g after-select-window replacement extra' >"$string_extra"
probe string-extra 1 \
    "$string_extra:1: command set-hook: too many arguments (need at most 2)" ''

monitor_arity="$work/monitor-arity.conf"
printf '%s\n' \
    'set-hook -g -B "@monitor::#{session_name}" { display one } { display two } { display three }' \
    >"$monitor_arity"
probe monitor-arity 1 \
    "$monitor_arity:1: command set-hook: too many arguments (need at most 2)" ''

child_precedence="$work/child-precedence.conf"
printf '%s\n' \
    'set-hook -g { no-such-set-hook-child } display-message' \
    >"$child_precedence"
probe child-precedence 1 \
    "$child_precedence:1: unknown command: no-such-set-hook-child" ''

canonical="$work/canonical.conf"
printf '%s\n' \
    'set-hook -gu after-select-window' \
    'set-hook -g after-select-window { display -p canonical }' \
    'show-hooks -g after-select-window' >"$canonical"
probe canonical 0 'after-select-window[0] display-message -p canonical' ''

outer_aliases="$work/outer-aliases.conf"
printf '%s\n' \
    'set-hook -gu after-select-window' \
    'set-h -g after-select-window[10] { display -p outer-prefix }' \
    'zzhook84 after-select-window[11] { display -p outer-user }' \
    'show-hooks -g after-select-window' >"$outer_aliases"
probe outer-aliases 0 'after-select-window[10] display-message -p outer-prefix
after-select-window[11] display-message -p outer-user' ''

inner_aliases="$work/inner-aliases.conf"
printf '%s\n' \
    'set-hook -gu after-select-window' \
    'set-hook -g after-select-window[20] { display -p inner-builtin }' \
    'set-hook -g after-select-window[21] { display-mes -p inner-prefix }' \
    'set-hook -g after-select-window[22] { zzdisplay84 inner-user }' \
    'show-hooks -g after-select-window' >"$inner_aliases"
probe inner-aliases 0 'after-select-window[20] display-message -p inner-builtin
after-select-window[21] display-message -p inner-prefix
after-select-window[22] display-message -p inner-user' ''

groups="$work/groups.conf"
printf '%s\n' \
    'set-hook -g @same { display -p one ; list-s }' \
    'set-hook -g @multi {' \
    '  display -p first' \
    '  list-s' \
    '}' \
    'set-hook -g @empty { }' \
    'show-hooks -g @same' \
    'show-hooks -g @multi' \
    'show-hooks -g @empty' >"$groups"
probe groups 0 "@same \"display-message -p one ; list-sessions\"
@multi \"display-message -p first ;; list-sessions\"
@empty ''" ''

builtin_groups="$work/builtin-groups.conf"
printf '%s\n' \
    'set-hook -g after-select-window {' \
    '  display -p first' \
    '  list-s' \
    '}' \
    'show-hooks -g after-select-window' >"$builtin_groups"
probe builtin-groups 0 \
    'after-select-window[0] display-message -p first ; list-sessions' ''

builtin_empty="$work/builtin-empty.conf"
printf '%s\n' \
    'set-hook -g after-select-window { }' \
    'show-hooks -g after-select-window' >"$builtin_empty"
probe builtin-empty 0 'after-select-window' ''

main_client set-environment -gu SET_HOOK_INHERITED
main_client set-hook -g after-select-window \
    'set-environment -g SET_HOOK_INHERITED fired'
local_empty="$work/local-empty.conf"
printf '%s\n' \
    'set-hook -a after-select-window { }' \
    'show-hooks after-select-window' >"$local_empty"
probe local-empty 0 'after-select-window' ''
main_client set-hook -R after-select-window
expect_absent_environment local-empty-shadow SET_HOOK_INHERITED

main_client set-hook -u after-select-window
local_parse_failure="$work/local-parse-failure.conf"
printf '%s\n' \
    'set-hook -a after-select-window "{ display -p invalid }"' \
    >"$local_parse_failure"
probe local-parse-failure 1 '' 'syntax error'
if [ "$(main_client show-hooks after-select-window)" != 'after-select-window' ]; then
    fail_check local-parse-failure-shadow
fi
main_client set-hook -R after-select-window
expect_absent_environment local-parse-failure-effect SET_HOOK_INHERITED
main_client set-hook -u after-select-window

main_client set-hook -g after-select-window 'display-message -p cleared-quoted'
quoted_builtin="$work/quoted-builtin.conf"
printf '%s\n' \
    'set-hook -g after-select-window "{ display -p quoted }"' \
    >"$quoted_builtin"
probe quoted-builtin 1 '' 'syntax error'
expect_hook quoted-builtin-cleared after-select-window 'after-select-window'

main_client set-hook -g 'after-select-window[7]' \
    'display-message -p preserved-indexed'
quoted_indexed="$work/quoted-indexed.conf"
printf '%s\n' \
    'set-hook -g after-select-window[7] "{ display -p quoted }"' \
    >"$quoted_indexed"
probe quoted-indexed 1 '' 'syntax error'
expect_hook quoted-indexed-preserved after-select-window \
    'after-select-window[7] display-message -p preserved-indexed'

quoted_custom="$work/quoted-custom.conf"
printf '%s\n' \
    'set-hook -g @quoted "{ display -p quoted }"' \
    'show-hooks -g @quoted' >"$quoted_custom"
probe quoted-custom 0 '@quoted "{ display -p quoted }"' ''

custom_run="$work/custom-run.conf"
printf '%s\n' \
    'set-hook -g @typed-run { display -p typed-run }' \
    'set-hook -g @string-run "display -p string-run"' \
    'set-hook -gR @typed-run' \
    'set-hook -gR @string-run' \
    'set-hook -gR @quoted' >"$custom_run"
probe custom-run 0 'typed-run
string-run' ''

main_client set-environment -gu SET_HOOK_RUN_NOW
main_client set-hook -g after-select-window \
    'set-environment -g SET_HOOK_RUN_NOW fired'
run_now_typed="$work/run-now-typed.conf"
printf '%s\n' \
    'set-hook -gR after-select-window { no-such-run-now-child }' \
    >"$run_now_typed"
probe run-now-typed 1 \
    "$run_now_typed:1: unknown command: no-such-run-now-child" ''
expect_absent_environment run-now-typed-effect SET_HOOK_RUN_NOW
run_now_string="$work/run-now-string.conf"
printf '%s\n' \
    'set-hook -gR after-select-window "no-such-run-now-child"' \
    >"$run_now_string"
probe run-now-string 0 '' ''
if [ "$(main_client show-environment -g SET_HOOK_RUN_NOW)" != \
    'SET_HOOK_RUN_NOW=fired' ]; then
    fail_check run-now-string-effect
fi

forwarded="$work/forwarded.conf"
printf '%s\n' \
    'set-hook default-client-command { display -p forwarded ; neww -d }' \
    'show-options -sv default-client-command' >"$forwarded"
probe forwarded 0 'display-message -p forwarded ; new-window -d' ''

stored="$work/stored.conf"
printf '%s\n' \
    'bind-key -T prefix F10 set-hook -g @stored { display -p stored ; list-s }' \
    >"$stored"
probe stored 0 '' ''
if [ "$(main_client list-keys -T prefix -F '#{key_string}=#{key_command}' F10)" != \
    'F10=set-hook -g @stored { display-message -p stored ; list-sessions }' ]; then
    fail_check stored-command-kind
fi

check_count=$((check_count + 1))
after_queue_marker="$work/after-queue.marker"
: >"$after_queue_marker"
main_client set-hook -g after-queue \
    "run-shell 'printf x >> \"$after_queue_marker\"'"
main_client display-message -p queue-one \; \
    display-message -p queue-two >/dev/null
if [ -s "$after_queue_marker" ]; then
    fail_check after-queue-ordinary
fi
main_client set-hook -gR after-queue >/dev/null
if [ "$(wc -c <"$after_queue_marker")" -ne 1 ]; then
    fail_check after-queue-manual-first
fi
main_client display-message -p ordinary-after >/dev/null
if [ "$(wc -c <"$after_queue_marker")" -ne 1 ]; then
    fail_check after-queue-ordinary-after
fi
main_client set-hook -gR after-queue >/dev/null
if [ "$(wc -c <"$after_queue_marker")" -ne 2 ]; then
    fail_check after-queue-manual-second
fi
main_client set-hook -gu after-queue

check_count=$((check_count + 1))
main_client set-environment -gu SET_HOOK_CONTROL_FORBIDDEN
control_reject_raw="$work/control-reject.raw"
control_reject_error="$work/control-reject.err"
{
    printf '%s\n' \
        'set-hook -g { set-environment -g SET_HOOK_CONTROL_FORBIDDEN yes } display-message'
    printf '%s\n' 'detach-client'
} | control_client >"$control_reject_raw" 2>"$control_reject_error" || true
control_accept_raw="$work/control-accept.raw"
control_accept_error="$work/control-accept.err"
set +e
printf '%s\n' 'set-hook -g @control { display -p control }' |
    control_client >"$control_accept_raw" 2>"$control_accept_error"
control_accept_status=$?
set -e
if [ "$control_accept_status" -ne 0 ] || [ -s "$control_reject_error" ] ||
    [ -s "$control_accept_error" ] ||
    ! grep -Fq 'command set-hook: argument 1 must be "string"' "$control_reject_raw" ||
    ! grep -Eq '^%error [0-9]+ [0-9]+ 1$' "$control_reject_raw" ||
    [ "$(main_client show-hooks -g @control)" != \
        '@control "display-message -p control"' ]; then
    fail_check control-typed-arguments
fi
expect_absent_environment control-typed-effect SET_HOOK_CONTROL_FORBIDDEN

if [ "$check_count" -ne 25 ]; then
    fail_check "check-count-$check_count"
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g ARGS_PARSE_SET_HOOK clean:25
else
    failure_labels="$(paste -sd, "$work/failures")"
    failure_side="${ZZ_SMOKE_CANARY:-missing-canary}"
    main_client set-environment -g ARGS_PARSE_SET_HOOK \
        "failed:$failure_side:$failure_labels"
    printf '%s:%s\n' "$failure_side" "$failure_labels" >&2
fi
