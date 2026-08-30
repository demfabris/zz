#!/bin/sh
set -efu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

work="$HOME/strict-key-literal-controls-work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0
control_names='[NUL] [SOH] [STX] [ETX] [EOT] [ENQ] [ASC] [BEL] [BS] Tab [LF] [VT] [FF] Enter [SO] [SI] [DLE] [DC1] [DC2] [DC3] [DC4] [NAK] [SYN] [ETB] [CAN] [EM] [SUB] Escape [FS] [GS] [RS] [US]'

control_name() {
    control_code="$1"
    set -- $control_names
    shift "$control_code"
    printf '%s\n' "$1"
}

control_argument() {
    control_octal="$(printf '%03o' "$1")"
    printf '%b' "^\\${control_octal}x"
}

probe() {
    label="$1"
    expected_status="$2"
    expected_output="$3"
    shift 3
    check_count=$((check_count + 1))
    output_file="$work/$check_count.out"
    error_file="$work/$check_count.err"
    expected_file="$work/$check_count.expected"
    if [ -n "$expected_output" ]; then
        printf '%s\n' "$expected_output" >"$expected_file"
    else
        : >"$expected_file"
    fi
    set +e
    main_client "$@" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    if [ "$status" -ne "$expected_status" ] || [ -s "$error_file" ] ||
        ! cmp -s "$output_file" "$expected_file"; then
        failed=1
        printf '%s\n' "$label" >>"$work/failures"
    fi
}

probe_missing() {
    label="$1"
    shift
    check_count=$((check_count + 1))
    output_file="$work/$check_count.out"
    error_file="$work/$check_count.err"
    set +e
    main_client "$@" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    if [ "$status" -ne 1 ] || [ -s "$output_file" ] || [ ! -s "$error_file" ]; then
        failed=1
        printf '%s\n' "$label" >>"$work/failures"
    fi
}

prefix_before="$(main_client show-options -gv prefix)"
prefix2_before="$(main_client show-options -gv prefix2)"
backspace_before="$(main_client show-options -sv backspace)"

probe sentinel-bind 0 '' \
    bind-key -T zzliteral C-a display-message sentinel

code=1
while [ "$code" -le 31 ]; do
    key="$(control_argument "$code")"
    key="${key%x}"
    rendered="C-$(control_name "$code")"
    probe "bind-$code" 0 '' \
        bind-key -T zzliteral "$key" display-message "literal-$code"
    probe "list-$code" 0 "$rendered|display-message literal-$code" \
        list-keys -T zzliteral -F '#{key_string}|#{key_command}' "$key"
    probe "unbind-$code" 0 '' unbind-key -T zzliteral "$key"
    probe_missing "missing-$code" \
        list-keys -T zzliteral -F '#{key_string}|#{key_command}' "$key"
    code=$((code + 1))
done

probe hex-literal-a-bind 0 '' \
    bind-key -T zzhex A display-message literal-a
probe hex-a-bind 0 '' \
    bind-key -T zzhex 0x41 display-message hex-a
probe hex-literal-space-bind 0 '' \
    bind-key -T zzhex Space display-message literal-space
probe hex-space-bind 0 '' \
    bind-key -T zzhex 0x20 display-message hex-space
probe hex-list-all 0 'Space|display-message literal-space
A|display-message literal-a
 |display-message hex-space
A|display-message hex-a' \
    list-keys -T zzhex -F '#{key_string}|#{key_command}'
probe hex-literal-a-filter 0 'A|display-message literal-a' \
    list-keys -T zzhex -F '#{key_string}|#{key_command}' A
probe hex-a-filter 0 'A|display-message hex-a' \
    list-keys -T zzhex -F '#{key_string}|#{key_command}' 0x41
probe hex-literal-space-filter 0 'Space|display-message literal-space' \
    list-keys -T zzhex -F '#{key_string}|#{key_command}' Space
probe hex-space-filter 0 ' |display-message hex-space' \
    list-keys -T zzhex -F '#{key_string}|#{key_command}' 0x20
probe hex-a-unbind 0 '' unbind-key -T zzhex 0x41
probe hex-literal-a-remains 0 'A|display-message literal-a' \
    list-keys -T zzhex -F '#{key_string}|#{key_command}' A
probe_missing hex-a-missing \
    list-keys -T zzhex -F '#{key_string}|#{key_command}' 0x41
probe hex-literal-space-unbind 0 '' unbind-key -T zzhex Space
probe hex-space-remains 0 ' |display-message hex-space' \
    list-keys -T zzhex -F '#{key_string}|#{key_command}' 0x20
probe hex-literal-a-unbind 0 '' unbind-key -T zzhex A
probe hex-space-unbind 0 '' unbind-key -T zzhex 0x20

probe hex-prefix-set 0 '' set-option -g prefix 0x41
probe hex-prefix-show 0 A show-options -gv prefix
probe hex-prefix2-set 0 '' set-option -g prefix2 0x20
probe hex-prefix2-show 0 ' ' show-options -gv prefix2
probe hex-backspace-set 0 '' set-option -s backspace 0x41
probe hex-backspace-show 0 A show-options -sv backspace

tab="$(control_argument 9)"
tab="${tab%x}"
enter="$(control_argument 13)"
enter="${enter%x}"
escape="$(control_argument 27)"
escape="${escape%x}"
probe prefix-set 0 '' set-option -g prefix "$tab"
probe prefix-show 0 C-Tab show-options -gv prefix
probe prefix2-set 0 '' set-option -g prefix2 "$enter"
probe prefix2-show 0 C-Enter show-options -gv prefix2
probe backspace-set 0 '' set-option -s backspace "$escape"
probe backspace-show 0 C-Escape show-options -sv backspace

probe prefix-restore 0 '' set-option -g prefix "$prefix_before"
probe prefix2-restore 0 '' set-option -g prefix2 "$prefix2_before"
probe backspace-restore 0 '' set-option -s backspace "$backspace_before"
probe sentinel-show 0 'C-a|display-message sentinel' \
    list-keys -T zzliteral -F '#{key_string}|#{key_command}' C-a
probe prefix-restored 0 "$prefix_before" show-options -gv prefix
probe prefix2-restored 0 "$prefix2_before" show-options -gv prefix2
probe backspace-restored 0 "$backspace_before" show-options -sv backspace
probe table-cleanup 0 '' unbind-key -a -T zzliteral

if [ "$check_count" -ne 161 ]; then
    failed=1
    printf '%s\n' count-mismatch >>"$work/failures"
fi

if [ "$failed" -eq 0 ]; then
    printf '%s\n' strict-key-literal-controls:clean
else
    printf 'strict-key-literal-controls:broken:%s\n' "${ZZ_SMOKE_CANARY:-missing}"
    while IFS= read -r failure; do
        printf 'failure:%s\n' "$failure"
    done <"$work/failures"
fi
