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

cases="$HOME/command-flag-errors.tsv"
work="$HOME/command-flag-errors-work"
mkdir -p "$work"
: >"$work/failures"

failed=0
probe_count=0
failure_probe_count=0
success_probe_count=0

probe_failure() {
    label="$1"
    expected="$2"
    shift 2
    probe_count=$((probe_count + 1))
    failure_probe_count=$((failure_probe_count + 1))
    output_file="$work/$probe_count.out"
    error_file="$work/$probe_count.err"
    expected_file="$work/$probe_count.expected"
    printf '%s\n' "$expected" >"$expected_file"
    set +e
    main_client "$@" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    if [ "$status" -ne 1 ] || [ -s "$output_file" ] ||
        ! cmp -s "$error_file" "$expected_file"; then
        failed=1
        printf '%s\n' "$label" >>"$work/failures"
    fi
}

probe_success_line() {
    label="$1"
    expected="$2"
    shift 2
    probe_count=$((probe_count + 1))
    success_probe_count=$((success_probe_count + 1))
    output_file="$work/$probe_count.out"
    error_file="$work/$probe_count.err"
    expected_file="$work/$probe_count.expected"
    printf '%s\n' "$expected" >"$expected_file"
    set +e
    main_client "$@" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    if [ "$status" -ne 0 ] || [ -s "$error_file" ] ||
        ! cmp -s "$output_file" "$expected_file"; then
        failed=1
        printf '%s\n' "$label" >>"$work/failures"
    fi
}

state_file="$HOME/command-flag-errors-output"
printf 'file sentinel\n' >"$state_file"
main_client set-buffer -b command-flag-errors-buffer 'buffer sentinel'
main_client bind-key -T prefix F12 display-message command-flag-errors-binding
main_client set-hook -g after-select-pane 'display-message command-flag-errors-hook'
main_client display-message -p '#{pane_id}' >"$work/pane.before"
main_client show-buffer -b command-flag-errors-buffer >"$work/buffer.before"
main_client list-keys -T prefix F12 >"$work/binding.before"
main_client show-hooks -g after-select-pane >"$work/hook.before"

canonical_count=0
alias_count=0
required_count=0
tab="$(printf '\t')"
while IFS="$tab" read -r canonical alias required usage; do
    if [ "$usage" = '@EMPTY@' ]; then
        usage=''
    fi
    canonical_count=$((canonical_count + 1))
    unknown="command $canonical: unknown flag -0"
    probe_failure "unknown-$canonical" "$unknown" "$canonical" -0
    if [ "$alias" != '-' ]; then
        alias_count=$((alias_count + 1))
        probe_failure "unknown-$alias" "$unknown" "$alias" -0
    fi
    probe_failure "punctuation-$canonical" \
        "command $canonical: invalid flag -@" "$canonical" '-@'
    probe_failure "long-$canonical" \
        "command $canonical: invalid flag --" "$canonical" --bogus
    probe_failure "help-$canonical" "usage: $canonical $usage" "$canonical" '-?'
    if [ "$required" != '-' ]; then
        required_count=$((required_count + 1))
        probe_failure "missing-$canonical-$required" \
            "command $canonical: $required expects an argument" \
            "$canonical" "$required"
    fi
done <"$cases"

probe_success_line required-help-value '-?' display-message -p -F '-?'
probe_success_line required-boundary-value '--' display-message -p -F --
probe_success_line required-attached-value '-?' display-message '-pF-?'
probe_failure required-then-unknown \
    'command display-message: unknown flag -0' display-message -F '-?' -0

for direction in -D -L -R -U; do
    probe_failure "optional-value-$direction-help" 'adjustment invalid' \
        resize-pane "$direction" '-?'
    probe_failure "optional-lookahead-$direction-long" \
        'command resize-pane: invalid flag --' resize-pane "$direction" --bogus
    probe_failure "optional-consumed-$direction-then-unknown" \
        'command resize-pane: unknown flag -0' resize-pane "$direction" 1 -0
done

probe_failure unsupported-attach-then-unknown \
    'command attach-session: unknown flag -0' attach-session -x -0
probe_failure unsupported-capture-then-unknown \
    'command capture-pane: unknown flag -0' capture-pane -C -0
probe_failure unsupported-move-then-unknown \
    'command move-pane: unknown flag -0' move-pane -M -0
probe_failure unsupported-required-then-unknown \
    'command break-pane: unknown flag -0' break-pane -X value -0
probe_failure unsupported-optional-then-invalid \
    'command move-pane: invalid flag --' move-pane -D --bogus

probe_failure first-positional-stops-flags \
    'command display-message: too many arguments (need at most 1)' \
    display-message value -0 extra
probe_failure explicit-boundary-stops-flags \
    'command kill-server: too many arguments (need at most 0)' \
    kill-server -- -0
probe_failure bare-dash-stops-flags \
    'command kill-server: too many arguments (need at most 0)' \
    kill-server - -0
probe_failure clustered-help \
    'usage: list-sessions [-r] [-F format] [-f filter] [-O order]' \
    list-sessions '-r?'
probe_failure leading-clustered-help \
    'usage: list-sessions [-r] [-F format] [-f filter] [-O order]' \
    list-sessions '-?0'

probe_failure attach-alias-punctuation \
    'command attach-session: invalid flag -@' attach '-@'
probe_failure attach-alias-long \
    'command attach-session: invalid flag --' attach --bogus
probe_failure attach-alias-help \
    'usage: attach-session [-dErx] [-c working-directory] [-f flags] [-t target-session]' \
    attach '-?'
probe_failure attach-missing-target \
    'command attach-session: -t expects an argument' attach-session -t
probe_failure attach-alias-missing-target \
    'command attach-session: -t expects an argument' attach -t

main_client display-message -p '#{pane_id}' >"$work/pane.after"
main_client show-buffer -b command-flag-errors-buffer >"$work/buffer.after"
main_client list-keys -T prefix F12 >"$work/binding.after"
main_client show-hooks -g after-select-pane >"$work/hook.after"
printf 'file sentinel\n' >"$work/file.expected"
if ! cmp -s "$work/pane.before" "$work/pane.after" ||
    ! cmp -s "$work/buffer.before" "$work/buffer.after" ||
    ! cmp -s "$work/binding.before" "$work/binding.after" ||
    ! cmp -s "$work/hook.before" "$work/hook.after" ||
    ! cmp -s "$state_file" "$work/file.expected"; then
    failed=1
fi

if [ "$canonical_count" -ne 83 ] || [ "$alias_count" -ne 74 ] ||
    [ "$required_count" -ne 79 ] || [ "$failure_probe_count" -ne 513 ] ||
    [ "$success_probe_count" -ne 3 ] || [ "$probe_count" -ne 516 ]; then
    failed=1
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COMMAND_FLAG_ERRORS clean:516
fi
