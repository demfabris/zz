#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
    initial_control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" -C "$@"
    }
    attached_control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" \
            -C attach-session -t =w
    }
else
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
    initial_control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" -C "$@"
    }
    attached_control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" \
            -C attach-session -t =w
    }
fi

work="$HOME/source-replay-diagnostics-work"
source_path="$work/source.conf"
command_source_path="$work/command-source.conf"
nested_source_path="$work/nested-source.conf"
alias_source_path="$work/alias-source.conf"
hook_combined_path="$work/hook-combined.conf"
hook_only_path="$work/hook-only.conf"
hook_source_child_path="$work/hook-source-child.conf"
hook_source_outer_path="$work/hook-source-outer.conf"
control_callback_path="$work/control-callback.conf"
control_runtime_path="$work/control-runtime.conf"
control_read_path="$work/control-read"
control_mixed_child_path="$work/control-mixed-child.conf"
control_mixed_outer_path="$work/control-mixed-outer.conf"
control_parser_path="$work/control-parser.conf"
control_confirm_child_path="$work/control-confirm-child.conf"
mkdir -p "$work"
main_client set-option -s 'command-alias[100]' 'rr=run-shell -C'
main_client set-option -s 'command-alias[101]' \
    'aliasif=if-shell -F 1 "{ display-message -p quoted }" ; display-message -p ALIAS_IF_SAME'
main_client set-option -s 'command-alias[102]' \
    'aliasrun=run-shell -C "{ display-message -p quoted }" ; display-message -p ALIAS_RUN_SAME'
main_client set-option -s 'command-alias[103]' \
    'aliasifleaf=if-shell -F 1 "{ display-message -p quoted }" ; set-environment -g ALIAS_IF_BODY_SAME yes'
main_client set-option -s 'command-alias[104]' \
    'aliasrunleaf=run-shell -C "{ display-message -p quoted }" ; set-environment -g ALIAS_RUN_BODY_SAME yes'
printf '%s\n' \
    'run-shell -C "{ display-message -p quoted }"' \
    'display-message -p ABBREVIATED_INNER_AFTER' >"$nested_source_path"
printf '%s\n' \
    'display-message -p DIRECT_BEFORE ; if-shell -F 1 "{ display-message -p quoted }" ; display-message -p DIRECT_SAME' \
    'display-message -p DIRECT_LATER' \
    'display-message -p NESTED_BEFORE ; if-shell -F 1 '\''if-shell -F 1 "{ display-message -p quoted }"'\'' ; display-message -p NESTED_SAME' \
    'display-message -p NESTED_LATER' \
    'display-message -p RUN_BEFORE ; run-shell -C "{ display-message -p quoted }" ; display-message -p RUN_SAME' \
    'display-message -p RUN_LATER' \
    'run-shell -C "display-message -p ALIAS_INNER_BEFORE ; rr \"{ display-message -p quoted }\" ; display-message -p ALIAS_INNER_SAME"' \
    'display-message -p ALIAS_LATER' \
    'run-shell -C { run-shell -C "{ display-message -p quoted }" ; run-shell -C "{ display-message -p quoted }" }' \
    'display-message -p MULTI_LATER' \
    'aliasif' \
    'display-message -p ALIAS_IF_LATER' \
    'aliasrun' \
    'display-message -p ALIAS_RUN_LATER' \
    "source-f '$nested_source_path'" \
    'display-message -p ABBREVIATED_OUTER_AFTER' >"$source_path"
printf '%s\n' \
    'run-shell -C "set-environment -g SOURCE_ALIAS_BEFORE yes ; rr \"{ display-message -p quoted }\" ; set-environment -g SOURCE_ALIAS_SAME yes"' \
    'run-shell -C { run-shell -C "{ display-message -p quoted }" ; run-shell -C "{ display-message -p quoted }" }' \
    'display-message -p COMMAND_AFTER' >"$command_source_path"
printf '%s\n' \
    'set-environment -g ALIAS_IF_BEFORE yes ; aliasifleaf ; set-environment -g ALIAS_IF_CALLER_SAME yes' \
    'set-environment -g ALIAS_IF_LATER yes' \
    'run-shell -C "set-environment -g ALIAS_RUN_BEFORE yes ; aliasrunleaf ; set-environment -g ALIAS_RUN_CALLER_SAME yes"' \
    'set-environment -g ALIAS_RUN_LATER yes' >"$alias_source_path"
printf '%s\n' \
    'run-shell -C "display-message -p TRIGGER ; run-shell -C \"{ display-message -p quoted }\""' \
    >"$hook_combined_path"
printf '%s\n' \
    'display-message -p TRIGGER' \
    'display-message -p AFTER' >"$hook_only_path"
printf '%s\n' \
    'run-shell -C "{ display-message -p quoted }"' \
    'display-message -p CHILD_LATER' >"$hook_source_child_path"
printf '%s\n' \
    'display-message -p SOURCE_HOOK_TRIGGER' >"$hook_source_outer_path"
printf '%s\n' \
    'run-shell -C "{ display-message -p quoted }"' \
    'display-message -p CHILD_LATER' >"$control_callback_path"
printf '%s\n' \
    'kill-session -t missing-control-runtime' \
    'display-message -p CHILD_RUNTIME_LATER' >"$control_runtime_path"
mkdir -p "$control_read_path"
printf '%s\n' \
    'run-shell -C "{ display-message -p quoted }"' >"$control_mixed_child_path"
printf '%s\n' \
    'if-shell -F 1 "{ display-message -p quoted }"' >"$control_mixed_outer_path"
printf '%s\n' \
    '%endif' \
    'display-message -p PARSE_LATER' >"$control_parser_path"
printf '%s\n' \
    'run-shell -C "{ display-message -p quoted }"' \
    'display-message -p CONFIRM_CHILD_LATER' >"$control_confirm_child_path"

control_callback_body="display-message -p OUTER_BEFORE ; source-file '$control_callback_path' ; display-message -p OUTER_AFTER"
control_runtime_body="display-message -p OUTER_BEFORE ; source-file '$control_runtime_path' ; display-message -p OUTER_AFTER"
control_read_body="display-message -p OUTER_BEFORE ; source-file '$control_read_path' ; display-message -p OUTER_AFTER"
main_client set-option -s 'command-alias[105]' \
    "controlcallback=$control_callback_body"
main_client set-option -s 'command-alias[106]' \
    "controlruntimerun=run-shell -C \"$control_runtime_body\""
main_client set-option -s 'command-alias[107]' \
    "controlreadrun=run-shell -C \"$control_read_body\""

wait_for_marker() {
    marker_path=$1
    marker_value=$2
    marker_pid=$3
    marker_attempt=0
    until grep -Fqx "$marker_value" "$marker_path" 2>/dev/null; do
        marker_attempt=$((marker_attempt + 1))
        if [ "$marker_attempt" -ge 400 ] || ! kill -0 "$marker_pid" 2>/dev/null; then
            return 1
        fi
        sleep 0.01
    done
}

wait_for_process() {
    wait_pid=$1
    wait_attempt=0
    while kill -0 "$wait_pid" 2>/dev/null && [ "$wait_attempt" -lt 200 ]; do
        wait_attempt=$((wait_attempt + 1))
        sleep 0.01
    done
    if kill -0 "$wait_pid" 2>/dev/null; then
        kill -TERM "$wait_pid" 2>/dev/null || true
    fi
    wait "$wait_pid"
}

normalize_transcript() {
    normalize_raw=$1
    normalize_first_flags=$2
    normalize_marker=$3
    sed -e "s|$source_path|<SOURCE>|g" \
        -e "s|$nested_source_path|<NESTED>|g" "$normalize_raw" | awk \
        -v first_flags="$normalize_first_flags" \
        -v marker="$normalize_marker" '
        function emit(terminator) {
            if (payload == "") payload = "_"
            if (source_frames < 31) {
                source_frames++
                printf "%s%d:%s:%s:%s", separator, source_frames, flags, terminator, payload
                separator = ";"
            } else if (payload == marker && flags == 1 && terminator == "end") {
                printf "%s34:%s:end:%s", separator, flags, payload
                separator = ";"
            } else {
                printf "%sunexpected:%s:%s:%s", separator, flags, terminator, payload
                separator = ";"
            }
            active = 0
            payload = ""
        }
        /^%begin [0-9]+ [0-9]+ [0-9]+$/ {
            if (!started && $4 != first_flags) next
            started = 1
            active = 1
            flags = $4
            payload = ""
            next
        }
        /^%end [0-9]+ [0-9]+ [0-9]+$/ {
            if (active) emit("end")
            next
        }
        /^%error [0-9]+ [0-9]+ [0-9]+$/ {
            if (active) emit("error")
            next
        }
        active && !/^%/ {
            if (payload != "") payload = payload "+"
            payload = payload $0
            next
        }
        started && !active && !/^%/ && $0 != "" {
            printf "%sraw:%s", separator, $0
            separator = ";"
        }
        END { print "" }
    '
}

run_probe() {
    probe_mode=$1
    probe_marker=$2
    probe_raw="$work/$probe_mode.raw"
    probe_error="$work/$probe_mode.err"
    probe_input="$work/$probe_mode.in"
    : >"$probe_raw"
    : >"$probe_error"
    rm -f -- "$probe_input"
    mkfifo "$probe_input"
    if [ "$probe_mode" = argv ]; then
        initial_control_client source-file "$source_path" \
            <"$probe_input" >"$probe_raw" 2>"$probe_error" &
        probe_first_flags=0
    else
        attached_control_client <"$probe_input" >"$probe_raw" 2>"$probe_error" &
        probe_first_flags=1
    fi
    probe_pid=$!
    exec 3>"$probe_input"
    if [ "$probe_mode" = stdin ]; then
        printf "source-file '%s'\n" "$source_path" >&3
        printf "display-message -p '%s'\n" "$probe_marker" >&3
        marker_status=0
        wait_for_marker "$probe_raw" "$probe_marker" "$probe_pid" || marker_status=$?
    else
        marker_status=0
    fi
    exec 3>&-
    probe_status=0
    wait_for_process "$probe_pid" || probe_status=$?
    probe_transcript="$(normalize_transcript \
        "$probe_raw" "$probe_first_flags" "$probe_marker")"
    if [ "$marker_status" -ne 0 ] || [ "$probe_status" -ne 1 ] || [ -s "$probe_error" ]; then
        printf '%s' broken
        return
    fi
    expected="1:$probe_first_flags:end:_;2:$probe_first_flags:end:DIRECT_BEFORE;3:$probe_first_flags:error:<SOURCE>:1: syntax error;4:$probe_first_flags:end:DIRECT_LATER;5:$probe_first_flags:end:NESTED_BEFORE;6:$probe_first_flags:end:_;7:$probe_first_flags:error:<SOURCE>:3: syntax error;8:$probe_first_flags:end:NESTED_SAME;9:$probe_first_flags:end:NESTED_LATER;10:$probe_first_flags:end:RUN_BEFORE;11:$probe_first_flags:end:_;raw:<SOURCE>:5: syntax error;12:$probe_first_flags:end:RUN_SAME;13:$probe_first_flags:end:RUN_LATER;14:$probe_first_flags:end:_;15:$probe_first_flags:end:ALIAS_INNER_BEFORE;16:$probe_first_flags:end:_;raw:<SOURCE>:7: syntax error;17:$probe_first_flags:end:ALIAS_INNER_SAME;18:$probe_first_flags:end:ALIAS_LATER;19:$probe_first_flags:end:_;20:$probe_first_flags:end:_;raw:<SOURCE>:9: syntax error;21:$probe_first_flags:end:_;raw:<SOURCE>:9: syntax error;22:$probe_first_flags:end:MULTI_LATER;23:$probe_first_flags:error:<SOURCE>:11: syntax error;24:$probe_first_flags:end:ALIAS_IF_LATER;25:$probe_first_flags:end:_;raw:<SOURCE>:13: syntax error;26:$probe_first_flags:end:ALIAS_RUN_SAME;27:$probe_first_flags:end:ALIAS_RUN_LATER;28:$probe_first_flags:end:_;29:$probe_first_flags:end:_;raw:<NESTED>:1: syntax error;30:$probe_first_flags:end:ABBREVIATED_INNER_AFTER;31:$probe_first_flags:end:ABBREVIATED_OUTER_AFTER"
    if [ "$probe_mode" = stdin ]; then
        expected="$expected;34:1:end:$probe_marker"
    fi
    if [ "$probe_transcript" = "$expected" ]; then
        printf '%s' clean
    else
        printf '%s' broken
    fi
}

run_command_probe() {
    command_output="$work/command.out"
    command_error="$work/command.err"
    command_status=0
    main_client source-file "$command_source_path" \
        >"$command_output" 2>"$command_error" || command_status=$?
    command_stdout="$(sed "s|$command_source_path|<COMMAND_SOURCE>|g" "$command_output")"
    command_stderr="$(sed "s|$command_source_path|<COMMAND_SOURCE>|g" "$command_error")"
    command_alias_before="$(main_client show-environment -g SOURCE_ALIAS_BEFORE 2>/dev/null || true)"
    command_alias_same="$(main_client show-environment -g SOURCE_ALIAS_SAME 2>/dev/null || true)"
    expected_stderr="$(printf '%s\n%s\n%s' \
        '<COMMAND_SOURCE>:1: syntax error' \
        '<COMMAND_SOURCE>:2: syntax error' \
        '<COMMAND_SOURCE>:2: syntax error')"
    if [ "$command_status" -eq 1 ] \
        && [ "$command_stdout" = COMMAND_AFTER ] \
        && [ "$command_stderr" = "$expected_stderr" ] \
        && [ "$command_alias_before" = SOURCE_ALIAS_BEFORE=yes ] \
        && [ "$command_alias_same" = SOURCE_ALIAS_SAME=yes ]; then
        printf '%s' clean
    else
        printf '%s' broken
    fi
}

environment_value() {
    environment_name=$1
    main_client show-environment -g "$environment_name" 2>/dev/null || true
}

run_alias_leaf_probe() {
    alias_output="$work/alias.out"
    alias_error="$work/alias.err"
    for alias_name in \
        ALIAS_IF_BEFORE \
        ALIAS_IF_BODY_SAME \
        ALIAS_IF_CALLER_SAME \
        ALIAS_IF_LATER \
        ALIAS_RUN_BEFORE \
        ALIAS_RUN_BODY_SAME \
        ALIAS_RUN_CALLER_SAME \
        ALIAS_RUN_LATER; do
        main_client set-environment -gu "$alias_name"
    done
    alias_status=0
    main_client source-file "$alias_source_path" \
        >"$alias_output" 2>"$alias_error" || alias_status=$?
    alias_error_count="$(grep -c 'syntax error' "$alias_error" || true)"
    if [ "$alias_status" -eq 1 ] \
        && [ ! -s "$alias_output" ] \
        && [ "$alias_error_count" -eq 2 ] \
        && [ "$(environment_value ALIAS_IF_BEFORE)" = ALIAS_IF_BEFORE=yes ] \
        && [ -z "$(environment_value ALIAS_IF_BODY_SAME)" ] \
        && [ -z "$(environment_value ALIAS_IF_CALLER_SAME)" ] \
        && [ "$(environment_value ALIAS_IF_LATER)" = ALIAS_IF_LATER=yes ]; then
        alias_if_result=clean
    else
        alias_if_result=broken
    fi
    if [ "$alias_status" -eq 1 ] \
        && [ ! -s "$alias_output" ] \
        && [ "$alias_error_count" -eq 2 ] \
        && [ "$(environment_value ALIAS_RUN_BEFORE)" = ALIAS_RUN_BEFORE=yes ] \
        && [ "$(environment_value ALIAS_RUN_BODY_SAME)" = ALIAS_RUN_BODY_SAME=yes ] \
        && [ "$(environment_value ALIAS_RUN_CALLER_SAME)" = ALIAS_RUN_CALLER_SAME=yes ] \
        && [ "$(environment_value ALIAS_RUN_LATER)" = ALIAS_RUN_LATER=yes ]; then
        alias_run_result=clean
    else
        alias_run_result=broken
    fi
    printf 'if:%s,run:%s' "$alias_if_result" "$alias_run_result"
}

normalize_hook_transcript() {
    hook_raw=$1
    hook_first_flags=$2
    sed -e "s|$hook_combined_path|<HOOK_COMBINED>|g" \
        -e "s|$hook_only_path|<HOOK_ONLY>|g" \
        -e "s|$hook_source_child_path|<HOOK_CHILD>|g" \
        -e "s|$hook_source_outer_path|<HOOK_OUTER>|g" "$hook_raw" | awk \
        -v first_flags="$hook_first_flags" '
        function emit(terminator) {
            if (payload == "") payload = "_"
            printf "%s%s:%s:%s", separator, flags, terminator, payload
            separator = ";"
            active = 0
            payload = ""
        }
        /^%begin [0-9]+ [0-9]+ [0-9]+$/ {
            if (!started && $4 != first_flags) next
            started = 1
            active = 1
            flags = $4
            payload = ""
            next
        }
        /^%end [0-9]+ [0-9]+ [0-9]+$/ {
            if (active) emit("end")
            next
        }
        /^%error [0-9]+ [0-9]+ [0-9]+$/ {
            if (active) emit("error")
            next
        }
        active && !/^%/ {
            if (payload != "") payload = payload "+"
            payload = payload $0
            next
        }
        started && !active && !/^%/ && $0 != "" {
            printf "%sraw:%s", separator, $0
            separator = ";"
        }
        END { print "" }
    '
}

run_hook_control_probe() {
    hook_mode=$1
    hook_name=$2
    hook_source=$3
    hook_expected=$4
    hook_raw="$work/$hook_name-$hook_mode.raw"
    hook_error="$work/$hook_name-$hook_mode.err"
    hook_input="$work/$hook_name-$hook_mode.in"
    : >"$hook_raw"
    : >"$hook_error"
    rm -f -- "$hook_input"
    mkfifo "$hook_input"
    if [ "$hook_mode" = argv ]; then
        initial_control_client source-file "$hook_source" \
            <"$hook_input" >"$hook_raw" 2>"$hook_error" &
        hook_first_flags=0
    else
        attached_control_client <"$hook_input" >"$hook_raw" 2>"$hook_error" &
        hook_first_flags=1
    fi
    hook_pid=$!
    exec 3>"$hook_input"
    if [ "$hook_mode" = stdin ]; then
        printf "source-file '%s'\n" "$hook_source" >&3
    fi
    exec 3>&-
    hook_status=0
    wait_for_process "$hook_pid" || hook_status=$?
    hook_transcript="$(normalize_hook_transcript "$hook_raw" "$hook_first_flags")"
    if [ "$hook_status" -eq 1 ] \
        && [ ! -s "$hook_error" ] \
        && [ "$hook_transcript" = "$hook_expected" ]; then
        printf '%s' clean
    else
        printf '%s' broken
    fi
}

run_hook_command_probe() {
    hook_name=$1
    hook_source=$2
    hook_expected_output=$3
    hook_expected_error=$4
    hook_output="$work/$hook_name-command.out"
    hook_error="$work/$hook_name-command.err"
    hook_status=0
    main_client source-file "$hook_source" \
        >"$hook_output" 2>"$hook_error" || hook_status=$?
    hook_stdout="$(sed \
        -e "s|$hook_combined_path|<HOOK_COMBINED>|g" \
        -e "s|$hook_only_path|<HOOK_ONLY>|g" \
        -e "s|$hook_source_child_path|<HOOK_CHILD>|g" \
        -e "s|$hook_source_outer_path|<HOOK_OUTER>|g" "$hook_output")"
    hook_stderr="$(sed \
        -e "s|$hook_combined_path|<HOOK_COMBINED>|g" \
        -e "s|$hook_only_path|<HOOK_ONLY>|g" \
        -e "s|$hook_source_child_path|<HOOK_CHILD>|g" \
        -e "s|$hook_source_outer_path|<HOOK_OUTER>|g" "$hook_error")"
    if [ "$hook_status" -eq 1 ] \
        && [ "$hook_stdout" = "$hook_expected_output" ] \
        && [ "$hook_stderr" = "$hook_expected_error" ]; then
        printf '%s' clean
    else
        printf '%s' broken
    fi
}

run_hook_callback_probes() {
    main_client set-hook -g after-display-message \
        'run-shell -C "{ display-message -p quoted }"'
    hook_combined_command="$(run_hook_command_probe \
        combined "$hook_combined_path" \
        TRIGGER \
        "$(printf '%s\n%s' 'syntax error' '<HOOK_COMBINED>:1: syntax error')")"
    hook_combined_argv="$(run_hook_control_probe \
        argv combined "$hook_combined_path" \
        '0:end:_;0:end:_;0:end:TRIGGER;0:end:_;raw:syntax error;0:end:_;raw:<HOOK_COMBINED>:1: syntax error')"
    hook_combined_stdin="$(run_hook_control_probe \
        stdin combined "$hook_combined_path" \
        '1:end:_;1:end:_;1:end:TRIGGER;0:end:_;raw:syntax error;1:end:_;raw:<HOOK_COMBINED>:1: syntax error')"
    hook_only_command="$(run_hook_command_probe \
        hook-only "$hook_only_path" \
        "$(printf '%s\n%s' TRIGGER AFTER)" \
        "$(printf '%s\n%s' 'syntax error' 'syntax error')")"
    hook_only_argv="$(run_hook_control_probe \
        argv hook-only "$hook_only_path" \
        '0:end:_;0:end:TRIGGER;0:end:_;raw:syntax error;0:end:AFTER;0:end:_;raw:syntax error')"
    hook_only_stdin="$(run_hook_control_probe \
        stdin hook-only "$hook_only_path" \
        '1:end:_;1:end:TRIGGER;0:end:_;raw:syntax error;1:end:AFTER;0:end:_;raw:syntax error')"
    printf 'combined-command:%s,combined-argv:%s,combined-stdin:%s,hook-only-command:%s,hook-only-argv:%s,hook-only-stdin:%s' \
        "$hook_combined_command" \
        "$hook_combined_argv" \
        "$hook_combined_stdin" \
        "$hook_only_command" \
        "$hook_only_argv" \
        "$hook_only_stdin"
}

run_hook_source_probes() {
    main_client set-hook -g after-display-message \
        "source-file '$hook_source_child_path'"
    hook_source_command="$(run_hook_command_probe \
        hook-source "$hook_source_outer_path" \
        "$(printf '%s\n%s' SOURCE_HOOK_TRIGGER CHILD_LATER)" \
        '<HOOK_CHILD>:1: syntax error')"
    hook_source_argv="$(run_hook_control_probe \
        argv hook-source "$hook_source_outer_path" \
        '0:end:_;0:end:SOURCE_HOOK_TRIGGER;0:end:_;0:end:_;raw:<HOOK_CHILD>:1: syntax error;0:end:CHILD_LATER')"
    hook_source_stdin="$(run_hook_control_probe \
        stdin hook-source "$hook_source_outer_path" \
        '1:end:_;1:end:SOURCE_HOOK_TRIGGER;0:end:_;0:end:_;raw:<HOOK_CHILD>:1: syntax error;0:end:CHILD_LATER')"
    printf 'command:%s,argv:%s,stdin:%s' \
        "$hook_source_command" "$hook_source_argv" "$hook_source_stdin"
}

normalize_exact_control_transcript() {
    exact_raw=$1
    sed -e "s|$control_callback_path|<CALLBACK>|g" \
        -e "s|$control_runtime_path|<RUNTIME>|g" \
        -e "s|$control_read_path|<READ>|g" \
        -e "s|$control_mixed_child_path|<CHILD>|g" \
        -e "s|$control_mixed_outer_path|<OUTER>|g" \
        -e "s|$control_parser_path|<PARSER>|g" \
        -e "s|$control_confirm_child_path|<CONFIRM_CHILD>|g" "$exact_raw" | awk '
        function append(value) {
            printf "%s%s", separator, value
            separator = ";"
        }
        function emit(terminator) {
            if (payload == "") payload = "_"
            append(flags ":" terminator ":" payload)
            active = 0
            payload = ""
        }
        /^%begin [0-9]+ [0-9]+ [0-9]+$/ {
            started = 1
            active = 1
            flags = $4
            payload = ""
            next
        }
        /^%end [0-9]+ [0-9]+ [0-9]+$/ {
            if (active) emit("end")
            next
        }
        /^%error [0-9]+ [0-9]+ [0-9]+$/ {
            if (active) emit("error")
            next
        }
        /^%config-error / {
            line = $0
            sub(/^%config-error /, "", line)
            append("config:" line)
            next
        }
        active && !/^%/ {
            line = $0
            if (index(line, "<READ>") != 0) line = "<READ_ERROR>"
            if (payload != "") payload = payload "+"
            payload = payload line
            next
        }
        started && !active && !/^%/ && $0 != "" {
            line = $0
            if (index(line, "<READ>") != 0) line = "<READ_ERROR>"
            append("raw:" line)
            next
        }
        END { print "" }
    '
}

normalize_exact_control_stderr() {
    exact_error=$1
    if [ ! -s "$exact_error" ]; then
        printf '%s' _
        return
    fi
    sed -e "s|$control_callback_path|<CALLBACK>|g" \
        -e "s|$control_runtime_path|<RUNTIME>|g" \
        -e "s|$control_read_path|<READ>|g" \
        -e "s|$control_mixed_child_path|<CHILD>|g" \
        -e "s|$control_mixed_outer_path|<OUTER>|g" \
        -e "s|$control_parser_path|<PARSER>|g" \
        -e "s|$control_confirm_child_path|<CONFIRM_CHILD>|g" "$exact_error" | awk '
        {
            line = $0
            if (index(line, "<READ>") != 0) line = "<READ_ERROR>"
            printf "%s%s", separator, line
            separator = "+"
        }
    '
}

run_exact_control_probe() {
    exact_name=$1
    shift
    exact_raw="$work/$exact_name.raw"
    exact_error="$work/$exact_name.err"
    exact_input="$work/$exact_name.in"
    : >"$exact_raw"
    : >"$exact_error"
    rm -f -- "$exact_input"
    mkfifo "$exact_input"
    initial_control_client "$@" \
        <"$exact_input" >"$exact_raw" 2>"$exact_error" &
    exact_pid=$!
    exec 3>"$exact_input"
    exec 3>&-
    exact_status=0
    wait "$exact_pid" || exact_status=$?
    exact_transcript="$(normalize_exact_control_transcript "$exact_raw")"
    exact_stderr="$(normalize_exact_control_stderr "$exact_error")"
    printf 'rc:%s,stderr:%s,events:%s' \
        "$exact_status" "$exact_stderr" "$exact_transcript"
}

publish_exact_control_probe() {
    exact_environment=$1
    shift
    exact_result="$(run_exact_control_probe "$@")"
    main_client set-environment -g "$exact_environment" "$exact_result"
}

run_confirm_control_probe() {
    confirm_target_log="$work/confirm-target.log"
    confirm_target_output="$work/confirm-target.out"
    confirm_target_error="$work/confirm-target.err"
    confirm_command_raw="$work/confirm-command.raw"
    confirm_command_error="$work/confirm-command.err"
    : >"$confirm_target_log"
    : >"$confirm_target_output"
    : >"$confirm_target_error"
    : >"$confirm_command_raw"
    : >"$confirm_command_error"
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        set -- "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" \
            attach-session -t =w
    else
        set -- "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" \
            attach-session -t =w
    fi
    env -u TMUX -u TMUX_PANE TERM=xterm-256color \
        python3 - "$confirm_target_log" "$@" \
        >"$confirm_target_output" \
        2>"$confirm_target_error" <<'PY' &
import errno
import os
import pty
import select
import signal
import sys
import time

log_path = sys.argv[1]
command = sys.argv[2:]
if not command:
    raise SystemExit(64)

pid, master = pty.fork()
if pid == 0:
    try:
        os.execvpe(command[0], command, os.environ.copy())
    except OSError:
        os._exit(127)

send_at = time.monotonic() + 1
deadline = time.monotonic() + 15
sent = False
timed_out = False
status = None
with open(log_path, "wb") as log:
    while status is None:
        now = time.monotonic()
        if now >= deadline:
            timed_out = True
            os.kill(pid, signal.SIGTERM)
            _, status = os.waitpid(pid, 0)
            break
        timeout = 0.05 if sent else min(0.05, max(0, send_at - now))
        readable, _, _ = select.select([master], [], [], timeout)
        if readable:
            try:
                data = os.read(master, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                data = b""
            if data:
                log.write(data)
                log.flush()
        if not sent and time.monotonic() >= send_at:
            try:
                os.write(master, b"y")
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
            sent = True
        waited, status = os.waitpid(pid, os.WNOHANG)
        if waited == 0:
            status = None

os.close(master)
if timed_out:
    raise SystemExit(2)
raise SystemExit(os.waitstatus_to_exitcode(status))
PY
    confirm_target_pid=$!
    confirm_target_attempt=0
    confirm_target_name=
    until [ -n "$confirm_target_name" ]; do
        confirm_target_name="$(main_client list-clients -F '#{client_name}' 2>/dev/null | head -n 1)"
        confirm_target_attempt=$((confirm_target_attempt + 1))
        if [ "$confirm_target_attempt" -ge 400 ] \
            || ! kill -0 "$confirm_target_pid" 2>/dev/null; then
            wait "$confirm_target_pid" || true
            printf '%s' target-broken
            return
        fi
        sleep 0.01
    done
    confirm_body="source-file '$control_confirm_child_path' ; display-message -p CONFIRM_INNER_AFTER"
    confirm_outer="confirm-before -t '$confirm_target_name' -p CONFIRM_PROMPT \"$confirm_body\" ; display-message -p CONFIRM_OUTER_AFTER"

    initial_control_client run-shell -C "$confirm_outer" \
        </dev/null \
        >"$confirm_command_raw" \
        2>"$confirm_command_error" &
    confirm_command_pid=$!
    confirm_command_attempt=0
    while kill -0 "$confirm_command_pid" 2>/dev/null \
        && [ "$confirm_command_attempt" -lt 500 ]; do
        confirm_command_attempt=$((confirm_command_attempt + 1))
        sleep 0.01
    done
    if kill -0 "$confirm_command_pid" 2>/dev/null; then
        confirm_command_status=124
        kill -TERM "$confirm_command_pid" 2>/dev/null || true
        wait "$confirm_command_pid" 2>/dev/null || true
    else
        confirm_command_status=0
        wait "$confirm_command_pid" || confirm_command_status=$?
    fi
    main_client detach-client -t "$confirm_target_name" >/dev/null 2>&1 || true
    main_client resize-window -x 80 -y 24 >/dev/null 2>&1 || true
    confirm_target_status=0
    wait "$confirm_target_pid" || confirm_target_status=$?

    confirm_transcript="$(normalize_exact_control_transcript "$confirm_command_raw")"
    confirm_stderr="$(normalize_exact_control_stderr "$confirm_command_error")"
    if [ "$confirm_target_status" -ne 0 ] \
        || [ -s "$confirm_target_error" ] \
        || [ -s "$confirm_target_output" ]; then
        printf 'pty-rc:%s,pty-streams:broken,request-rc:%s,stderr:%s,events:%s' \
            "$confirm_target_status" \
            "$confirm_command_status" \
            "$confirm_stderr" \
            "$confirm_transcript"
        return
    fi
    printf 'pty-rc:%s,pty-streams:_,request-rc:%s,stderr:%s,events:%s' \
        "$confirm_target_status" \
        "$confirm_command_status" \
        "$confirm_stderr" \
        "$confirm_transcript"
}

run_exact_control_probes() {
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_CALLBACK_RUN \
        callback-run run-shell -C "$control_callback_body"
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_CALLBACK_IF \
        callback-if if-shell -F 1 "$control_callback_body"
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_CALLBACK_ALIAS \
        callback-alias controlcallback

    main_client set-hook -g command-error \
        'display-message -p HOOK_ERROR_EVENT'
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_RUNTIME_RUN \
        runtime-run run-shell -C "$control_runtime_body"
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_RUNTIME_IF \
        runtime-if if-shell -F 1 "$control_runtime_body"
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_RUNTIME_ALIAS_RUN_SHELL \
        runtime-alias-run-shell controlruntimerun
    main_client set-hook -gu command-error

    publish_exact_control_probe SOURCE_REPLAY_CONTROL_READ_RUN \
        read-run run-shell -C "$control_read_body"
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_READ_IF \
        read-if if-shell -F 1 "$control_read_body"
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_READ_ALIAS_RUN_SHELL \
        read-alias-run-shell controlreadrun

    main_client set-hook -g command-error \
        "source-file '$control_mixed_child_path' ; run-shell -C \"{ display-message -p quoted }\""
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_MIXED_SOURCE_FIRST \
        mixed-source-first source-file "$control_mixed_outer_path"
    main_client set-hook -g command-error \
        "run-shell -C \"{ display-message -p quoted }\" ; source-file '$control_mixed_child_path'"
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_MIXED_DIRECT_FIRST \
        mixed-direct-first source-file "$control_mixed_outer_path"
    main_client set-hook -gu command-error

    publish_exact_control_probe SOURCE_REPLAY_CONTROL_PARSER \
        parser source-file "$control_parser_path"
    publish_exact_control_probe SOURCE_REPLAY_CONTROL_PARSER_NO_EXECUTE \
        parser-no-execute source-file -n "$control_parser_path"

    confirm_result="$(run_confirm_control_probe)"
    main_client set-environment -g SOURCE_REPLAY_CONTROL_CONFIRM_ACCEPTED \
        "$confirm_result"
}

command_result="$(run_command_probe)"
argv_result="$(run_probe argv ARGV_DONE)"
stdin_result="$(run_probe stdin STDIN_DONE)"
main_client set-environment -g SOURCE_REPLAY_CONTROL_DIAGNOSTICS \
    "command:$command_result,argv:$argv_result,stdin:$stdin_result"
alias_leaf_result="$(run_alias_leaf_probe)"
main_client set-environment -g SOURCE_REPLAY_ALIAS_LEAF_POLICY \
    "$alias_leaf_result"
hook_callback_result="$(run_hook_callback_probes)"
main_client set-environment -g SOURCE_REPLAY_HOOK_CALLBACKS \
    "$hook_callback_result"
hook_source_result="$(run_hook_source_probes)"
main_client set-environment -g SOURCE_REPLAY_HOOK_NESTED_SOURCE \
    "$hook_source_result"
main_client set-hook -gu after-display-message
run_exact_control_probes
