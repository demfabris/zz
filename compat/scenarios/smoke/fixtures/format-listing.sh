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

session=fmtlist
work="$HOME/format-listing-work-$side"
rm -rf "$work"
mkdir -p "$work/one"
: >"$work/failures"
failed=0
check_count=0
client_pid=""
step=0

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
    step=$((step + 1))
    echo quit >"$work/one/step-$step" 2>/dev/null
    if [ -n "$client_pid" ]; then
        kill "$client_pid" >/dev/null 2>&1
        wait "$client_pid" >/dev/null 2>&1
    fi
    main_client kill-session -t "=$session" >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

drive() {
    step=$((step + 1))
    printf '%s\n' "$1" >"$work/one/step-$step"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$work/one/ack-$step" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "drive-$step"
    return 0
}

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

listing() {
    main_client display-message -a -p -t "$pane" >"$work/$1" 2>"$work/$1.err"
}

names() {
    cut -d= -f1 "$work/$1"
}

# The 56 table names whose callback returns NULL for a pane with no client, and
# the 28 of those that stay NULL once a client is attached.
declines_without_client='buffer_created buffer_full buffer_name buffer_sample buffer_size client_activity client_cell_height client_cell_width client_colours client_control_mode client_created client_discarded client_flags client_height client_key_table client_last_session client_name client_pid client_prefix client_readonly client_session client_termfeatures client_termname client_termtype client_theme client_tty client_uid client_user client_utf8 client_width client_written mouse_hyperlink mouse_line mouse_pane mouse_status_line mouse_status_range mouse_word mouse_x mouse_y pane_dead_signal pane_dead_status pane_dead_time pane_mode pane_pipe_pid session_active session_attached_list session_group session_group_attached session_group_attached_list session_group_list session_group_many_attached session_group_size window_active_clients_list window_bigger window_offset_x window_offset_y'
declines_with_client='buffer_created buffer_full buffer_name buffer_sample buffer_size client_last_session mouse_hyperlink mouse_line mouse_pane mouse_status_line mouse_status_range mouse_word mouse_x mouse_y pane_dead_signal pane_dead_status pane_dead_time pane_mode pane_pipe_pid session_group session_group_attached session_group_attached_list session_group_list session_group_many_attached session_group_size window_offset_x window_offset_y'

absent_names() {
    for name in $2; do
        if ! grep -qx "$name" "$work/$1.names"; then
            printf '%s ' "$name"
        fi
    done
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n '1p')"

listing base
names base >"$work/base.names"

# format_each walks the 198-entry table in declaration order, which is
# alphabetical, skipping every NULL callback, and then the command's own tree.
check_equal base-count 143 "$(grep -c . "$work/base")"
check_equal base-stderr '' "$(cat "$work/base.err")"
check_equal base-table-sorted same \
    "$(if [ "$(sed '$d' "$work/base.names")" = "$(sed '$d' "$work/base.names" | sort)" ]; then echo same; else echo differs; fi)"
check_equal base-tree-last command=display-message "$(tail -n 1 "$work/base")"

# The declining set is exactly the pin's, name by name.
check_equal base-declines "$declines_without_client " "$(absent_names base "$declines_without_client")"

# A few values both binaries own outright, to prove the walk carries values and
# not just names. The two values that carry further `=` characters are
# client_mode_format and tree_mode_format, whose values belong to their own
# tracked gaps, so the shape is checked instead: every line is name=value split
# at the first `=`.
check_equal base-wrap "wrap_flag=1" "$(grep '^wrap_flag=' "$work/base")"
check_equal base-grouped "session_grouped=0" "$(grep '^session_grouped=' "$work/base")"
check_equal base-pipe "pane_pipe=0" "$(grep '^pane_pipe=' "$work/base")"
check_equal base-unseen "pane_unseen_changes=0" "$(grep '^pane_unseen_changes=' "$work/base")"
check_equal base-last-attached "session_last_attached=0" "$(grep '^session_last_attached=' "$work/base")"
check_equal base-pane-index "pane_index=0" "$(grep '^pane_index=' "$work/base")"
check_equal base-line-shape 143 "$(grep -c '^[a-z_0-9][a-z_0-9]*=' "$work/base")"

# -a runs before the template is looked at, so -l, a message and the missing -p
# all leave the listing alone, while the -F conflict is still refused first.
check_equal without-p 143 "$(main_client display-message -a -t "$pane" 2>/dev/null | grep -c .)"
check_equal literal 143 "$(main_client display-message -a -l -p -t "$pane" | grep -c .)"
check_equal with-message 143 "$(main_client display-message -a -p -t "$pane" hello | grep -c .)"
check_equal format-conflict 'only one of -F or argument must be given' \
    "$(main_client display-message -a -p -F x -t "$pane" msg 2>&1 >/dev/null)"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$work/one" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
client_pid=$!
await_clients 1 || { echo "format-listing-$side: attach"; exit 0; }

# c->theme is THEME_UNKNOWN until the terminal answers the query, and
# format_cb_client_theme returns NULL for that, so the client reports dark the
# way a terminal that supports \033[?2031h does.
drive "keys 1b5b3f3939373b316e"
sleep 0.6
check_equal theme dark "$(main_client display-message -p -t "$pane" '#{client_theme}')"

listing attached
names attached >"$work/attached.names"
check_equal attached-count 172 "$(grep -c . "$work/attached")"
check_equal attached-table-sorted same \
    "$(if [ "$(sed '$d' "$work/attached.names")" = "$(sed '$d' "$work/attached.names" | sort)" ]; then echo same; else echo differs; fi)"
check_equal attached-tree-last command=display-message "$(tail -n 1 "$work/attached")"
check_equal attached-declines "$declines_with_client " "$(absent_names attached "$declines_with_client")"

# The 28 names an attached client adds back are exactly the difference.
sort "$work/base.names" >"$work/base.sorted"
sort "$work/attached.names" >"$work/attached.sorted"
check_equal added-back \
    'client_activity client_cell_height client_cell_width client_colours client_control_mode client_created client_discarded client_flags client_height client_key_table client_name client_pid client_prefix client_readonly client_session client_termfeatures client_termname client_termtype client_theme client_tty client_uid client_user client_utf8 client_width client_written session_active session_attached_list window_active_clients_list window_bigger' \
    "$(comm -13 "$work/base.sorted" "$work/attached.sorted" | tr '\n' ' ' | sed 's/ $//')"
check_equal removed-by-client '' "$(comm -23 "$work/base.sorted" "$work/attached.sorted" | tr '\n' ' ' | sed 's/ $//')"
check_equal attached-theme "client_theme=dark" "$(grep '^client_theme=' "$work/attached")"
check_equal attached-bigger "window_bigger=0" "$(grep '^window_bigger=' "$work/attached")"
check_equal attached-active "session_active=1" "$(grep '^session_active=' "$work/attached")"

if [ "$check_count" -ne 26 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FORMAT_LISTING clean:26
else
    sed "s/^/format-listing-$side: /" "$work/failures"
fi
