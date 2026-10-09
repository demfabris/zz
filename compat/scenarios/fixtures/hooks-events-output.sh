#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" \
            -C attach-session -t w
    }
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" \
            -C attach-session -t w
    }
fi

work="$HOME/hooks-events-output-$side"
rm -rf "$work"
mkdir -p "$work"
seen=""

wait_option() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        value="$(main_client show-options -gqv "$1")"
        if [ -n "$value" ]; then
            seen="$seen $1=$value"
            return
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    seen="$seen $1=missing"
}

cat >"$work/emit.sh" <<'EMIT'
#!/bin/sh
while [ ! -e "$1" ]; do sleep 0.05; done
printf 'out\007'
printf '\033]133;A\007'
printf '\033]133;C\007'
printf '\033]133;D;3\007'
sleep 300
EMIT

pane="$(main_client split-window -d -P -F '#{pane_id}' -t w:0 "sh '$work/emit.sh' '$work/go'")"
main_client set-hook -p -t "$pane" pane-activity "set -gF @hev-activity '#{hook}/#{hook_event}/#{==:#{hook_pane},$pane}'"
main_client set-hook -p -t "$pane" pane-bell "set -gF @hev-bell '#{hook}/#{hook_event}/#{==:#{hook_pane},$pane}'"
main_client set-hook -p -t "$pane" pane-shell-prompt "set -gF @hev-prompt '#{hook}/#{hook_event}/#{==:#{hook_pane},$pane}'"
main_client set-hook -p -t "$pane" pane-command-started "set -gF @hev-started '#{hook}/#{hook_event}/#{==:#{hook_pane},$pane}/#{!=:#{hook_command_start_time},}/#{hook_command_status}'"
main_client set-hook -p -t "$pane" pane-command-finished "set -gF @hev-finished '#{hook}/#{hook_event}/#{hook_command_status}/#{!=:#{hook_command_end_time},}/#{!=:#{hook_command_duration},}'"
: >"$work/go"
wait_option @hev-activity
wait_option @hev-bell
wait_option @hev-prompt
wait_option @hev-started
wait_option @hev-finished
main_client kill-pane -t "$pane"

main_client set-hook -g client-created "set -gF @hev-created '#{hook}/#{hook_event}/#{hook_session_name}/#{hook_window_index}/#{!=:#{hook_pane},}'"
main_client set-hook -g client-closed "set -gF @hev-closed '#{hook}/#{hook_event}'"
control_client </dev/null >/dev/null 2>&1 || true
wait_option @hev-created
wait_option @hev-closed
main_client set-hook -gu client-created
main_client set-hook -gu client-closed

cat >"$work/burst.sh" <<'BURST'
#!/bin/sh
while [ ! -e "$1" ]; do sleep 0.05; done
printf '\033]133;A\007\033]133;C\007\033]133;D;1\007\033]133;A\007\033]133;C\007\033]133;D;2\007\033]133;A\007'
sleep 300
BURST
burst="$(main_client split-window -d -P -F '#{pane_id}' -t w:0 "sh '$work/burst.sh' '$work/burst-go'")"
main_client set-option -g @hev-burst ''
main_client set-hook -p -t "$burst" pane-shell-prompt "set -gaF @hev-burst 'A+'"
main_client set-hook -p -t "$burst" pane-command-started "set -gaF @hev-burst 'C+'"
main_client set-hook -p -t "$burst" pane-command-finished "set -gaF @hev-burst 'D#{hook_command_status}+'"
: >"$work/burst-go"
attempt=0
while [ "$attempt" -lt 200 ]; do
    case "$(main_client show-options -gqv @hev-burst)" in
        *D2+A+) break ;;
    esac
    attempt=$((attempt + 1))
    sleep 0.05
done
seen="$seen burst=$(main_client show-options -gqv @hev-burst)"
main_client kill-pane -t "$burst"

main_client set-option -t w @hev-flag 0
main_client set-option -g @hev-monlog ''
main_client set-hook -t w -T -B '@hevmon::#{@hev-flag}' "set -gaF @hev-monlog '#{hook_value}+'"
sleep 1.5
for value in 1 0 2; do
    main_client set-option -t w @hev-flag "$value"
    sleep 1.5
done
main_client set-hook -t w -u -B @hevmon
seen="$seen monitor=$(main_client show-options -gv @hev-monlog)"

main_client set-environment -g HOOKS_EVENTS_OUTPUT "$(echo $seen)"
