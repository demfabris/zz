#!/bin/sh
# tmux 3.8 ends an unquoted token at } and, outside a command block, that }
# is a syntax error: a #{...} format written mid-word without quotes fails
# the whole sourced file or control line, while quoted or escaped braces and
# a { with no } still parse.
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    side=tmux
    main_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

work="$HOME/parse-close-brace-$side"
mkdir -p "$work"

main_client new-session -d -s pcb -x 80 -y 24
main_client set-option -w -t pcb:0 automatic-rename off
main_client rename-window -t pcb:0 win

for line in \
    'display -p foo::#{window_name}' \
    'display -p a}' \
    'display -p }' \
    'display -p a{b}' \
    'display -p a#{window_name} ; display -p next' \
    'if -F 1 { display -p in#{window_name} }' \
    'display -p a{b' \
    'display -p "a}"' \
    'display -p a\}' \
    "display -p 'a#{window_name}'" \
    "if -F 1 { display -p 'in#{window_name}' }"; do
    printf '%s\n%s\n' 'display -p first' "$line" >"$work/one.conf"
    set +e
    out=$(main_client source-file "$work/one.conf" 2>&1)
    rc=$?
    set -e
    printf '%s => rc=%s %s\n' "$line" "$rc" \
        "$(printf '%s' "$out" | sed "s|$work/||g" | tr '\n' '|')"
done

set +e
printf '%s\n' \
    'refresh-client -B pcbsub::#{window_name}' \
    "refresh-client -B 'pcbsub::#{window_name}'" \
    'display -p a#{window_name}' \
    'display -p "a#{window_name}"' |
    main_client -C attach-session -t '=pcb' >"$work/control.raw" 2>&1
set -e
sed -E \
    -e '/^%(output|subscription-changed) /d' \
    -e 's/^%(begin|end|error) [0-9]+ [0-9]+ ([0-9]+)$/%\1 \2/' \
    "$work/control.raw" | sed 's/^/control | /'

main_client kill-session -t '=pcb'
main_client set-environment -g PARSE_CLOSE_BRACE reached-end
