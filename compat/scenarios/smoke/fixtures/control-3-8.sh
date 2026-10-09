#!/bin/sh
# Control-mode behaviour that moved with tmux 3.8. Every refresh-client -C
# resizes every sized window and fires its events as they happen, so each
# call sends %layout-change and runs window-resized hooks even when no size
# changed, and end of file no longer loses them. A blank line still ends the
# client before the commands read with it run, so their notifications are
# dropped. A read-only control client runs queries and refresh-client.
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

work="$HOME/control-3-8-$side"
rm -rf "$work"
mkdir -p "$work"

normalize() {
    sed -E \
        -e '/^%output /d' \
        -e '/^%window-renamed @[0-9]+ (bash|sh|zsh)$/d' \
        -e 's/^%(begin|end|error) [0-9]+ [0-9]+ ([0-9]+)$/%\1 \2/'
}

paced() {
    for line in "$@"; do
        printf '%s\n' "$line"
        sleep 0.4
    done
}

drive() {
    label="$1"
    mode="$2"
    shift 2
    raw="$work/$label.raw"
    err="$work/$label.err"
    set +e
    case "$mode" in
    paced) paced "$@" | main_client -C attach-session -t '=c38' >"$raw" 2>"$err" ;;
    readonly) paced "$@" | main_client -C attach-session -r -t '=c38' >"$raw" 2>"$err" ;;
    chunk) printf '%s\n' "$@" | main_client -C attach-session -t '=c38' >"$raw" 2>"$err" ;;
    esac
    rc=$?
    set -e
    printf '%s rc=%s\n' "$label" "$rc"
    normalize <"$raw" | sed "s/^/$label | /"
    if [ -s "$err" ]; then
        sed "s/^/$label ! /" "$err"
    fi
}

main_client new-session -d -s c38 -x 80 -y 24
main_client new-window -d -t c38
main_client set-option -g automatic-rename off
main_client rename-window -t c38:0 zero
main_client rename-window -t c38:1 one
main_client set-hook -g window-resized 'set-option -gF @c38_resized "#{@c38_resized}r"'
main_client set-hook -g window-layout-changed 'set-option -gF @c38_layout "#{@c38_layout}l"'

drive same-size paced \
    'refresh-client -C 80,24' \
    'display-message -p "first #{@c38_resized} #{@c38_layout}"' \
    'refresh-client -C 80,24' \
    'display-message -p "second #{@c38_resized} #{@c38_layout}"' \
    'refresh-client -C @1:70x20' \
    'refresh-client -C @1:' \
    'display-message -p "window #{@c38_resized} #{@c38_layout}"'

main_client set-hook -gu window-resized
main_client set-hook -gu window-layout-changed
main_client split-window -d -t c38:1
main_client resize-pane -Z -t c38:1.0
main_client set-hook -g window-zoomed 'set-option -gF @c38_zoom "#{@c38_zoom}z"'
main_client set-hook -g window-unzoomed 'set-option -gF @c38_zoom "#{@c38_zoom}u"'

drive zoomed paced \
    'refresh-client -C 80,24' \
    'refresh-client -C 100,30' \
    'display-message -p "zoom #{@c38_zoom}"'

main_client set-hook -gu window-zoomed
main_client set-hook -gu window-unzoomed

drive eof-chunk chunk \
    'refresh-client -C 90,30' \
    'refresh-client -C 90,30' \
    'rename-window -t c38:0 eof' \
    'display-message -p EOF'

drive blank-chunk chunk \
    'refresh-client -C 90,30' \
    'rename-window -t c38:0 blank' \
    'set-buffer -b c38 x' \
    'display-message -p BLANK' \
    ''

drive read-only readonly \
    'display-message -p "#{client_readonly} #{client_control_mode}"' \
    'list-windows -F "#{window_index}:#{window_name}"' \
    'show-options -g base-index' \
    'has-session -t c38' \
    'refresh-client -C 110,30' \
    "refresh-client -B 'c38sub::#{window_name}'" \
    'refresh-client -B c38sub' \
    'send-keys -t c38:0 "echo typed" Enter' \
    'refresh-client -f "!read-only"' \
    'display-message -p "#{client_readonly}"' \
    'switch-client -r' \
    'display-message -p "#{client_readonly} #{client_control_mode}"'

set +e
printf '%s\n' 'new-window -d -n ro-made' |
    main_client -C attach-session -r -t '=c38' >"$work/ruled.raw" 2>&1
set -e
made=$(main_client list-windows -t c38 -F '#{window_name}' | grep -c '^ro-made$' || true)
refused=$(grep -c '^client is read-only$' "$work/ruled.raw" || true)
case "$side:$made:$refused" in
tmux:1:0 | zz:0:1) verdict=as-ruled ;;
*) verdict="unexpected made=$made refused=$refused" ;;
esac
printf 'read-only-new-window %s (control-mode.read-only-commands: 3.8 runs it, zz refuses)\n' "$verdict"
main_client kill-window -t c38:ro-made >/dev/null 2>&1 || true

main_client set-option -w -t c38:0 window-size manual
manual_window=$(main_client display-message -p -t c38:0 '#{window_id}')
drive manual paced \
    "refresh-client -C $manual_window:" \
    'display-message -p manual'

main_client delete-buffer -b c38 >/dev/null 2>&1 || true
main_client kill-session -t '=c38'
main_client set-environment -g CONTROL_3_8 reached-end
