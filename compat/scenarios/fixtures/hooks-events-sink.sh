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

work="$HOME/hooks-events-sink-$side"
rm -rf "$work"
mkdir -p "$work"
seen=""

wait_waiters() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        count="$(main_client wait-for -E -l "$1" | wc -l | tr -d ' ')"
        if [ "$count" -ge "$2" ]; then
            return
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    seen="$seen waiters-$1=missing"
}

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

lines() {
    tr '\n' ';' <"$1"
}

main_client wait-for -E -v session-renamed >"$work/verbose" 2>&1 &
verbose=$!
wait_waiters session-renamed 1
main_client rename-session -t w hevsink
wait "$verbose" || true
main_client rename-session -t hevsink w
seen="$seen verbose=[$(lines "$work/verbose")]"

main_client wait-for -E -v -F '#{==:#{new_name},hevb}' session-renamed >"$work/filter" 2>&1 &
filter=$!
wait_waiters session-renamed 1
main_client rename-session -t w heva
main_client rename-session -t heva hevb
wait "$filter" || true
main_client rename-session -t hevb w
seen="$seen filter=[$(lines "$work/filter")]"

main_client wait-for -E after-select-window >"$work/after" 2>&1 &
after=$!
wait_waiters after-select-window 1
main_client select-window -t w:0
wait "$after" || true
seen="$seen after=[$(lines "$work/after")]"

main_client wait-for -E -v @hevuser >"$work/user" 2>&1 &
user=$!
wait_waiters @hevuser 1
main_client set-hook -E -t w:0 @hevother
main_client set-hook -E -t w:0 @hevuser
wait "$user" || true
seen="$seen user=[$(grep -E '^(event|session|window|window_index)=' "$work/user" | tr '\n' ';')]"

main_client wait-for -E session-renamed >"$work/woken" 2>&1 &
woken=$!
wait_waiters session-renamed 1
waiter="$(main_client wait-for -E -l session-renamed)"
main_client wait-for -E -w "$waiter" session-renamed
wait "$woken" || true
seen="$seen woken=[$(lines "$work/woken")]"
seen="$seen nobody=[$(main_client wait-for -E -w hev-nobody session-renamed 2>&1 || true)]"
seen="$seen invalid=[$(main_client wait-for -E hev-not-an-event 2>&1 || true)]"

finish_wait() {
    attempt=0
    while [ "$attempt" -lt 100 ] && kill -0 "$1" 2>/dev/null; do
        attempt=$((attempt + 1))
        sleep 0.05
    done
    if kill -0 "$1" 2>/dev/null; then
        seen="$seen $2=parked"
        main_client wait-for -E -w "$(main_client wait-for -E -l "$2" | head -n 1)" "$2" || true
    fi
    wait "$1" || true
}

main_client wait-for -E -v -F 0 session-renamed >"$work/stream" 2>&1 &
stream=$!
wait_waiters session-renamed 1
main_client rename-session -t w hevstream
attempt=0
while [ "$attempt" -lt 40 ] && [ ! -s "$work/stream" ]; do
    attempt=$((attempt + 1))
    sleep 0.05
done
seen="$seen streamed=[$(lines "$work/stream")]"
main_client rename-session -t hevstream w
main_client wait-for -E -w "$(main_client wait-for -E -l session-renamed | head -n 1)" session-renamed
wait "$stream" || true
seen="$seen stream-done=[$(lines "$work/stream")]"

mkfifo "$work/control-in"
control_client <"$work/control-in" >"$work/control" 2>&1 &
control=$!
exec 3>"$work/control-in"
printf 'wait-for -E -v -F 0 session-renamed\n' >&3
wait_waiters session-renamed 1
main_client rename-session -t w hevcontrol
attempt=0
while [ "$attempt" -lt 40 ] && ! grep -q '^event=' "$work/control"; do
    attempt=$((attempt + 1))
    sleep 0.05
done
seen="$seen control=[$(grep -E '^(event|new_name|old_name|session)=' "$work/control" | tr '\n' ';')]"
main_client rename-session -t hevcontrol w
main_client wait-for -E -w "$(main_client wait-for -E -l session-renamed | head -n 1)" session-renamed
exec 3>&-
wait "$control" || true

main_client wait-for -E -F '#{pane_id}' session-renamed >"$work/untargeted" 2>&1 &
untargeted=$!
wait_waiters session-renamed 1
main_client rename-session -t w hevuntargeted
sleep 0.3
if kill -0 "$untargeted" 2>/dev/null; then
    seen="$seen untargeted=parked"
else
    seen="$seen untargeted=woken"
fi
main_client rename-session -t hevuntargeted w
finish_wait "$untargeted" session-renamed

main_client set-option -t w @hevflag 0
main_client set-hook -t w -B '@hevwatch::#{@hevflag}' "set -gF @hevwatchlog '#{hook_value}'"
sleep 1.5
main_client wait-for -E -v @hevwatch >"$work/watch" 2>&1 &
watch=$!
wait_waiters @hevwatch 1
main_client set-option -t w @hevflag 1
finish_wait "$watch" @hevwatch
seen="$seen watch=[$(grep -E '^(event|last|value)=' "$work/watch" | tr '\n' ';')]"
main_client set-hook -t w -u -B @hevwatch
main_client set-option -t w -u @hevflag
main_client set-option -gu @hevwatchlog

main_client set-hook -g pane-exited "set -gF @hev-exited '#{hook}/#{hook_exit_status}/#{hook_exit_signal}/#{hook_exit_success}'"
main_client split-window -d -t w:0 'exit 3'
wait_option @hev-exited
main_client set-hook -gu pane-exited
main_client set-option -g remain-on-exit on
main_client set-hook -g pane-died "set -gF @hev-died '#{hook}/#{hook_exit_status}/#{hook_exit_signal}/#{hook_exit_success}'"
main_client split-window -d -t w:0 'exit 0'
wait_option @hev-died
main_client set-option -gu @hev-died
main_client kill-pane -t w:0.1
main_client split-window -d -t w:0 'kill -TERM $$'
wait_option @hev-died
main_client kill-pane -t w:0.1
main_client set-hook -gu pane-died
main_client set-option -gu remain-on-exit

main_client new-session -d -s hevother
main_client set-option -g @hev-changed ''
main_client set-hook -g client-session-changed "set -gaF @hev-changed '#{hook_old_session_name}>#{hook_new_session_name}/#{==:#{hook_session},#{hook_new_session}}+'"
printf 'switch-client -t hevother\n' | control_client >/dev/null 2>&1 || true
attempt=0
while [ "$attempt" -lt 200 ]; do
    case "$(main_client show-options -gqv @hev-changed)" in
        *'>hevother'*) break ;;
    esac
    attempt=$((attempt + 1))
    sleep 0.05
done
seen="$seen changed=$(main_client show-options -gqv @hev-changed)"
main_client set-hook -gu client-session-changed
main_client kill-session -t hevother

payload() {
    name=$1
    shift
    main_client wait-for -E -v "$name" >"$work/payload-$name" 2>&1 &
    pid=$!
    wait_waiters "$name" 1
    main_client "$@"
    finish_wait "$pid" "$name"
    seen="$seen $name=[$(lines "$work/payload-$name")]"
}

main_client rename-window -t w:0 hevwin
payload window-renamed rename-window -t w:0 hevwin2
payload pane-exited split-window -d -t w:0 'exit 3'
payload pane-title-changed select-pane -t w:0.0 -T hevtitle
payload window-linked new-window -d -t w:5
main_client kill-window -t w:5

main_client set-environment -g HOOKS_EVENTS_SINK "$(echo $seen)"
