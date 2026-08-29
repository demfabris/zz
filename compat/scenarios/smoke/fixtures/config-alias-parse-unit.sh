#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
    cold_socket="/tmp/zzcapu-$$.sock"
    cold_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$cold_socket" "$@"
    }
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
    cold_label="zzcapu-$$"
    cold_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$cold_label" "$@"
    }
fi

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    cold_client kill-server >/dev/null 2>&1
    if [ "$side" = zz ]; then
        rm -f -- "$cold_socket" "${cold_socket}.identity" "${cold_socket}.lock"
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

work="$HOME/config-alias-parse-unit-$side"
mkdir -p "$work"

flatten() {
    awk '
        BEGIN { separator = "" }
        { printf "%s%s", separator, $0; separator = "+" }
        END { print "" }
    ' "$1"
}

capture_source() {
    capture_label="$1"
    shift
    capture_out="$work/$capture_label.out"
    capture_err="$work/$capture_label.err"
    if main_client source-file "$@" >"$capture_out" 2>"$capture_err"; then
        capture_status=0
    else
        capture_status=$?
    fi
    if [ "$capture_status" -ne 0 ] || [ -s "$capture_err" ]; then
        printf error
    else
        flatten "$capture_out"
    fi
}

same_config="$work/same.conf"
main_client set-option -s 'command-alias[94]' \
    'zsame=display-message -p old'
printf '%s\n' \
    "set-option -s command-alias[94] 'zsame=display-message -p same-new' ; zsame" \
    >"$same_config"
same_result="$(capture_source same "$same_config")"

later_config="$work/later.conf"
main_client set-option -s 'command-alias[95]' \
    'zlater=display-message -p old'
printf '%s\n' \
    "set-option -s command-alias[95] 'zlater=display-message -p later-new'" \
    'zlater' \
    >"$later_config"
later_result="$(capture_source later "$later_config")"

batch_first="$work/batch-first.conf"
batch_child="$work/batch-child.conf"
main_client set-option -s 'command-alias[97]' \
    'zparse=display-message -p $ALIAS_PARSE_ENV'
printf '%s\n' \
    'ALIAS_PARSE_ENV=one' \
    'zparse' \
    "set-option -s command-alias[97] 'zparse=display-message -p nested-new'" \
    "source-file '$batch_child'" \
    >"$batch_first"
printf '%s\n' \
    'ALIAS_PARSE_ENV=two' \
    'zparse' \
    >"$batch_child"
batch_result="$(capture_source batch "$batch_first" "$batch_child")"

root_one="$work/root-one.conf"
root_two="$work/root-two.conf"
cold_out="$work/cold.out"
cold_err="$work/cold.err"
printf '%s\n' \
    "set-option -s command-alias[98] 'zroots=set-option -g @alias_root_seen seen'" \
    >"$root_one"
printf '%s\n' 'zroots' >"$root_two"
if cold_client -f "$root_one" -f "$root_two" \
    new-session -d -s roots >"$cold_out" 2>"$cold_err"; then
    cold_status=0
else
    cold_status=$?
fi
if [ "$cold_status" -ne 0 ] || [ -s "$cold_out" ] || [ -s "$cold_err" ]; then
    roots_result=error
else
    root_alias="$(cold_client show-options -sqv 'command-alias[98]' 2>/dev/null || :)"
    root_marker="$(cold_client show-options -gqv @alias_root_seen 2>/dev/null || :)"
    if [ "$root_alias" != 'zroots=set-option -g @alias_root_seen seen' ]; then
        roots_result=missing
    elif [ "$root_marker" = seen ]; then
        roots_result=seen
    else
        roots_result=unseen
    fi
fi

main_client set-environment -g CONFIG_ALIAS_PARSE_UNIT \
    "same:$same_result,later:$later_result,batch:$batch_result,roots:$roots_result"
