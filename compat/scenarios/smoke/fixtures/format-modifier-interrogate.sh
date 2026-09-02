#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    set -- --socket "$ZZ_SMOKE_ZZ_SOCKET"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    set -- -L "$ZZ_SMOKE_TMUX_LABEL"
fi
prefix_args="$*"
main_client() {
    # shellcheck disable=SC2086
    "$binary" $prefix_args "$@"
}

session=interrog
work="$HOME/format-modifier-interrogate-$side"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0
viewer_pid=""

record_failure() {
    failed=1
    echo "$1" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1 want=[$2] got=[$3]"
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$viewer_pid" ]; then
        kill "$viewer_pid" >/dev/null 2>&1
        wait "$viewer_pid" >/dev/null 2>&1
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

await_clients() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        count="$(main_client list-clients -t "=$session" -F x 2>/dev/null | grep -c x || true)"
        if [ "$count" = "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-clients-$1"
    return 1
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24 cat
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | head -n 1)"

# `format_replace` leaves the value empty for every I flag when the tree has no
# client, so a detached server answers nothing for all three.
check_equal absent-capability '' "$(main_client display-message -p -t "$pane" '#{I/c:smcup}')"
check_equal absent-feature '' "$(main_client display-message -p -t "$pane" '#{I/f:RGB}')"
check_equal absent-environment '' "$(main_client display-message -p -t "$pane" '#{I/e:FOO}')"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color COLORTERM=truecolor FOO=barvalue \
    python3 "$HOME/send-keys-attach.py" record "$work/viewer.raw" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/viewer.out" 2>&1 &
viewer_pid=$!
await_clients 1 || {
    echo "format-modifier-interrogate-$side: attach"
    exit 0
}
client="$(main_client list-clients -t "=$session" -F '#{client_name}' | head -n 1)"

value() {
    main_client display-message -p -t "$pane" -c "$client" "#{$1}"
}

check_equal termname xterm-256color "$(value client_termname)"

# `tty_term_has_name` walks tmux's own tty_term_codes table over the term
# object the client's terminfo entry built, so these come straight from the
# entry, extended section included.
for name in acsc am AX bce clear colors cup kf1 kf63 setab setaf smcup XT; do
    check_equal "entry-$name" 1 "$(value "I/c:$name")"
done

# `tty_apply_features` writes each enabled feature's capability list into the
# same term, so these answer 1 without being in the entry at all: Ss and Se from
# cstyle, Ms from clipboard, Cs and Cr from ccolour and tsl and fsl from title,
# all five turned on by the stock `terminal-features` `xterm*` row; Enbp, Dsbp,
# Enfcs and Dsfcs from the bpaste, focus and title `tty_term_create` adds for a
# terminal whose clear starts with CSI; and setrgbf and setrgbb from the RGB
# COLORTERM=truecolor asks for.
for name in Cr Cs Dsbp Dsfcs Enbp Enfcs fsl Ms Se setrgbb setrgbf Ss tsl; do
    check_equal "applied-$name" 1 "$(value "I/c:$name")"
done

# Capabilities in neither source answer 0, whether they are tmux extensions no
# enabled feature writes or plain terminfo names this entry omits.
for name in Clmg Cmg Dseks Dsmg Eneks Enmg Hls ol RGB Rect Smol Smulx Setulc \
    Spb Swd Sxl Sync smxx Tc U8; do
    check_equal "absent-$name" 0 "$(value "I/c:$name")"
done

# A name outside tty_term_codes answers 0 whatever the entry says: hs is a real
# terminfo boolean tmux never reads.
check_equal outside-the-table-hs 0 "$(value I/c:hs)"
check_equal outside-the-table-unknown 0 "$(value I/c:nosuchcap)"

# `tty_feature_present` answers the feature bit first, then falls back to every
# capability the feature names being present with its own term flags set, which
# is why 256 and mouse answer 1 for a client whose feature bits carry neither.
for name in 256 bpaste ccolour clipboard cstyle focus mouse RGB title; do
    check_equal "feature-$name" 1 "$(value "I/f:$name")"
done
for name in extkeys hyperlinks ignorefkeys margins nosuchfeature osc7 overline \
    progressbar rectfill sixel strikethrough sync usstyle; do
    check_equal "no-feature-$name" 0 "$(value "I/f:$name")"
done

# `environ_find` on the format tree's own client, which for a `-c` command is
# the target client and not the invoking one.
check_equal environment-hit barvalue "$(value I/e:FOO)"
check_equal environment-miss '' "$(value I/e:NOSUCHVAR)"

# `format_replace` runs the three checks in the fixed source order c, f, e and
# each overwrites the last, so a flag word naming several answers the last one
# that ran rather than the first one written.
check_equal order-c-then-f 1 "$(value I/cf:RGB)"
check_equal order-c-f-then-e barvalue "$(value I/cfe:FOO)"
check_equal order-f-then-e barvalue "$(value I/fe:FOO)"
check_equal order-is-not-source-order barvalue "$(value I/ec:FOO)"

# `case 'I'` breaks at `argc < 1`, and a flag word with none of c, f or e sets
# no bit, so both fall through to the ordinary lookup of the body.
check_equal missing-flag-word-falls-back "$pane" "$(value I:pane_id)"
check_equal unknown-flag-word-falls-back '' "$(value I/z:RGB)"

# The body is a literal capability name and is never expanded.
check_equal body-is-literal 0 "$(value 'I/c:#{l:smcup}')"
check_equal empty-capability-body 0 "$(value I/c:)"
check_equal empty-feature-body 0 "$(value I/f:)"

if [ "$check_count" -ne 85 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FORMAT_MODIFIER_INTERROGATE clean:85
else
    sed "s/^/format-modifier-interrogate-$side: /" "$work/failures"
fi
