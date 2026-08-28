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

work="$HOME/args-parse-set-option-work"
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

main_client set-option -s 'command-alias[90]' 'zzset84=set-option -g'
main_client set-option -s 'command-alias[91]' 'zzdm84=display-message -p'

main_client set-environment -gu SET_OPTION_NAME_FORBIDDEN
typed_name="$work/typed-name.conf"
printf '%s\n' \
    'set-option -g { set-environment -g SET_OPTION_NAME_FORBIDDEN yes } replacement' \
    >"$typed_name"
probe typed-name 1 \
    "$typed_name:1: command set-option: argument 1 must be \"string\"" ''
if main_client show-environment -g SET_OPTION_NAME_FORBIDDEN >/dev/null 2>&1; then
    failed=1
    printf '%s\n' typed-name-effect >>"$work/failures"
fi

main_client set-environment -gu SET_OPTION_TARGET_FORBIDDEN
typed_target="$work/typed-target.conf"
printf '%s\n' \
    'set-window-option -t { set-environment -g SET_OPTION_TARGET_FORBIDDEN yes } @typed replacement' \
    >"$typed_target"
probe typed-target 1 \
    "$typed_target:1: command set-window-option: -t argument must be a string" ''
if main_client show-environment -g SET_OPTION_TARGET_FORBIDDEN >/dev/null 2>&1; then
    failed=1
    printf '%s\n' typed-target-effect >>"$work/failures"
fi

typed_extra="$work/typed-extra.conf"
printf '%s\n' \
    'set-option @extra value { display-message -p forbidden-extra }' \
    >"$typed_extra"
probe typed-extra 1 \
    "$typed_extra:1: command set-option: argument 3 must be \"string\"" ''

string_extra="$work/string-extra.conf"
printf '%s\n' 'set-window-option @extra value extra' >"$string_extra"
probe string-extra 1 \
    "$string_extra:1: command set-window-option: too many arguments (need at most 2)" ''

canonical="$work/canonical.conf"
printf '%s\n' \
    'set-option -g @typed-canonical { display-message -p canonical }' \
    'show-options -gv @typed-canonical' >"$canonical"
probe canonical 0 'display-message -p canonical' ''

window="$work/window.conf"
printf '%s\n' \
    'set-window-option -g @typed-window { display-message -p window }' \
    'show-window-options -gv @typed-window' >"$window"
probe window 0 'display-message -p window' ''

outer_builtin="$work/outer-builtin.conf"
printf '%s\n' \
    'set -g @outer-builtin { display-message -p outer-builtin }' \
    'show-options -gv @outer-builtin' >"$outer_builtin"
probe outer-builtin 0 'display-message -p outer-builtin' ''

outer_prefix="$work/outer-prefix.conf"
printf '%s\n' \
    'set-o -g @outer-prefix { display-message -p outer-prefix }' \
    'show-options -gv @outer-prefix' >"$outer_prefix"
probe outer-prefix 0 'display-message -p outer-prefix' ''

outer_user="$work/outer-user.conf"
printf '%s\n' \
    'zzset84 @outer-user { display-message -p outer-user }' \
    'show-options -gv @outer-user' >"$outer_user"
probe outer-user 0 'display-message -p outer-user' ''

inner_builtin="$work/inner-builtin.conf"
printf '%s\n' \
    'set-option -g @inner-builtin { display -p inner-builtin }' \
    'show-options -gv @inner-builtin' >"$inner_builtin"
probe inner-builtin 0 'display-message -p inner-builtin' ''

inner_prefix="$work/inner-prefix.conf"
printf '%s\n' \
    'set-option -g @inner-prefix { display-mes -p inner-prefix }' \
    'show-options -gv @inner-prefix' >"$inner_prefix"
probe inner-prefix 0 'display-message -p inner-prefix' ''

inner_user="$work/inner-user.conf"
printf '%s\n' \
    'set-option -g @inner-user { zzdm84 inner-user }' \
    'show-options -gv @inner-user' >"$inner_user"
probe inner-user 0 'display-message -p inner-user' ''

boundaries="$work/boundaries.conf"
printf '%s\n' \
    'set-option -g -- @double-dash { display -p double-dash }' \
    'set-option @late -g' \
    'show-options -gv @double-dash' \
    'show-options -v @late' >"$boundaries"
probe boundaries 0 'display-message -p double-dash
-g' ''

quoted="$work/quoted.conf"
printf '%s\n' \
    'set-option -g @quoted "{ display -p quoted }"' \
    'show-options -gv @quoted' >"$quoted"
probe quoted 0 '{ display -p quoted }' ''

multi="$work/multi.conf"
printf '%s\n' \
    'set-option -g @multi { display -p one ; list-s }' \
    'set-option -g @multiline {' \
    '  display -p first' \
    '  list-s' \
    '}' \
    'set-option -g @nested { if-shell -F 1 { display hi } { display bye } }' \
    'show-options -gv @multi' \
    'show-options -gv @multiline' \
    'show-options -gv @nested' >"$multi"
probe multi 0 'display-message -p one ; list-sessions
display-message -p first ;; list-sessions
if-shell -F 1 { display-message hi } { display-message bye }' ''

empty="$work/empty.conf"
printf '%s\n' \
    'set-window-option -g @typed-empty { }' \
    "display-message -p 'empty=<#{@typed-empty}>'" >"$empty"
probe empty 0 'empty=<>' ''

format_off="$work/format-off.conf"
printf '%s\n' \
    'set-option -g "@#{session_name}" { display -p "#{session_name}" }' \
    'show-options -gv @w' >"$format_off"
probe format-off 0 'display-message -p "#{session_name}"' ''

format_on="$work/format-on.conf"
printf '%s\n' \
    'set-option -gF @typed-format { display -p "#{session_name}" }' \
    'show-options -gv @typed-format' >"$format_on"
probe format-on 0 'display-message -p "w"' ''

semantic="$work/semantic.conf"
printf '%s\n' \
    'set-option -s default-client-command { display -p client ; neww -d }' \
    'show-options -sv default-client-command' >"$semantic"
probe semantic 0 'display-message -p client ; new-window -d' ''

main_client set-environment -gu SET_OPTION_STORED_FORBIDDEN
stored="$work/stored.conf"
printf '%s\n' \
    'bind-key -T prefix F10 set-option -g @stored { display-message -p stored ; list-sessions }' \
    'bind-key -T prefix F10 set-option -g { set-environment -g SET_OPTION_STORED_FORBIDDEN yes } replacement' \
    >"$stored"
probe stored 1 '' 'command set-option: argument 1 must be "string"'
stored_expected='F10=set-option -g @stored { display-message -p stored ; list-sessions }'
if [ "$(main_client list-keys -T prefix -F '#{key_string}=#{key_command}' F10)" != \
    "$stored_expected" ]; then
    failed=1
    printf '%s\n' stored-command-kind >>"$work/failures"
fi
if main_client show-environment -g SET_OPTION_STORED_FORBIDDEN >/dev/null 2>&1; then
    failed=1
    printf '%s\n' stored-command-effect >>"$work/failures"
fi

check_count=$((check_count + 1))
main_client set-environment -gu SET_OPTION_CONTROL_FORBIDDEN
control_reject_raw="$work/control-reject.raw"
control_reject_error="$work/control-reject.err"
{
    printf '%s\n' \
        'set-option -g { set-environment -g SET_OPTION_CONTROL_FORBIDDEN yes } replacement'
    printf '%s\n' 'detach-client'
} | control_client >"$control_reject_raw" 2>"$control_reject_error" || true
control_accept_raw="$work/control-accept.raw"
control_accept_error="$work/control-accept.err"
set +e
printf '%s\n' 'set -g @control { display -p control }' |
    control_client >"$control_accept_raw" 2>"$control_accept_error"
control_accept_status=$?
set -e
if [ "$control_accept_status" -ne 0 ] || [ -s "$control_reject_error" ] ||
    [ -s "$control_accept_error" ] ||
    ! grep -Fq 'command set-option: argument 1 must be "string"' "$control_reject_raw" ||
    ! grep -Eq '^%error [0-9]+ [0-9]+ 1$' "$control_reject_raw" ||
    [ "$(main_client show-options -gv @control)" != 'display-message -p control' ]; then
    failed=1
    printf '%s\n' control-typed-arguments >>"$work/failures"
fi
if main_client show-environment -g SET_OPTION_CONTROL_FORBIDDEN >/dev/null 2>&1; then
    failed=1
    printf '%s\n' control-typed-effect >>"$work/failures"
fi

if [ "$failed" -eq 0 ] && [ "$check_count" -eq 21 ]; then
    main_client set-environment -g ARGS_PARSE_SET_OPTION clean:21
fi
