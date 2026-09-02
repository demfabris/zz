#!/bin/sh
set -eu

outer="$ZZ_SMOKE_TMUX_BIN"
if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    inner_socket="/tmp/zz-prompt-chain-zz-$$.sock"
    INNER() { env -u TMUX -u TMUX_PANE "$ZZ_SMOKE_ZZ_BIN" -f /dev/null -S "$inner_socket" "$@"; }
    attach="env -u TMUX -u TMUX_PANE $ZZ_SMOKE_ZZ_BIN -f /dev/null -S $inner_socket attach -t s"
    main_client() { "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"; }
else
    side=pin
    inner_socket="/tmp/zz-prompt-chain-pin-$$.sock"
    INNER() { env -u TMUX -u TMUX_PANE "$ZZ_SMOKE_TMUX_BIN" -f /dev/null -S "$inner_socket" "$@"; }
    attach="env -u TMUX -u TMUX_PANE $ZZ_SMOKE_TMUX_BIN -f /dev/null -S $inner_socket attach -t s"
    main_client() { "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"; }
fi
outer_label="zz-prompt-outer-$side-$$"
OUTER() { "$outer" -f /dev/null -L "$outer_label" "$@"; }

label() {
    OUTER capture-pane -p -t o 2>/dev/null | sed -n '24p' | cut -c1-20 |
        sed -e 's/[[:space:]]*$//'
}

report=""
run() {
    name="$1"
    binding="$2"
    answers="$3"
    rm -f "$inner_socket"
    INNER new-session -d -s s -x 80 -y 24 >/dev/null 2>&1 || true
    INNER set -g status-keys emacs >/dev/null 2>&1 || true
    eval "INNER bind-key -n F1 $binding" >/dev/null 2>&1 || true
    OUTER new-session -d -s o -x 80 -y 24 "$attach" >/dev/null 2>&1 || true
    sleep 2
    OUTER send-keys -t o F1 >/dev/null 2>&1 || true
    sleep 1
    report="$report $name[$(label)"
    old_ifs="$IFS"
    IFS=','
    set -- $answers
    IFS="$old_ifs"
    while [ "$#" -gt 0 ]; do
        case "$1" in
        -) ;;
        *) OUTER send-keys -t o -l -- "$1" >/dev/null 2>&1 || true ;;
        esac
        OUTER send-keys -t o Enter >/dev/null 2>&1 || true
        shift
        sleep 1
        if [ "$#" -gt 0 ]; then
            report="$report;$(label)"
        fi
    done
    result="$(INNER show-environment -g PROMPT_RESULT 2>&1 || true)"
    report="$report]=$result"
    OUTER kill-server >/dev/null 2>&1 || true
    INNER kill-server >/dev/null 2>&1 || true
    sleep 0.5
}

run chain \
    "command-prompt -p 'first,second' -I 'AA,BB' 'set-environment -g PROMPT_RESULT \"<%1|%2>\"'" \
    "-,-"
run single-line \
    "command-prompt -l -p 'a,b' -I 'X,Y' 'set-environment -g PROMPT_RESULT \"<%1>\"'" \
    "-"
run derived-label \
    "command-prompt -I 'AA' 'set-environment -g PROMPT_RESULT \"<%1>\"'" \
    "-"
run bare-label \
    "command-prompt -I 'A'" \
    "-"
run pass-order \
    "command-prompt -p 'a,b' 'set-environment -g PROMPT_RESULT \"<%1>\"'" \
    "%2,Z"
run repeated-percent \
    "command-prompt -p 'a,b' 'set-environment -g PROMPT_RESULT \"<%%|%%>\"'" \
    "P,Q"
run short-inputs \
    "command-prompt -p 'a,b,c' -I 'only' 'set-environment -g PROMPT_RESULT \"<%1|%2|%3>\"'" \
    "-,-,-"

main_client set-environment -g PROMPT_CHAIN "$report"
