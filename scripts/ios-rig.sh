#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
action="${1:-start}"
socket="${ZZ_IOS_RIG_SOCKET:-/tmp/zzios-$(basename "$repo_root").sock}"
home="$repo_root/target/ios-rig"
cli="$repo_root/target/debug/zz_cli"
unset ZZ_SOCKET ZZ_PANE ZZ_SESSION TMUX TMUX_PANE ZZ_TMUX_EXECUTABLE ZZ_APP_STARTUP_DIRECTORY ZZ_STARTUP_REENTRY ZZ_DEV_BUILD

rig() { env HOME="$home" XDG_CONFIG_HOME="$home/.config" XDG_DATA_HOME="$home/.local/share" ZZ_TRAY=0 "$cli" --socket "$socket" "$@"; }
alive() { [[ -x "$cli" ]] && rig list-sessions >/dev/null 2>&1; }

case "$action" in
    start)
        cargo build --quiet -p zz-cli
        if ! alive; then
            mkdir -p "$home"
            rm -f "$socket"
            nohup env HOME="$home" XDG_CONFIG_HOME="$home/.config" XDG_DATA_HOME="$home/.local/share" ZZ_TRAY=0 \
                "$cli" --socket "$socket" daemon > "$home/daemon.log" 2>&1 &
            for _ in $(seq 50); do alive && break; sleep 0.1; done
            alive || { echo "the rig daemon did not start; see $home/daemon.log" >&2; exit 1; }
        fi
        if ! rig has-session -t phone 2>/dev/null; then
            rig new-session -d -s phone
            rig split-window -h -t phone
            rig new-window -t phone
        fi
        echo "rig daemon on $socket (session phone); just ios run attaches to it"
        ;;
    stop)
        if alive; then rig kill-server; fi
        rm -f "$socket"
        echo "rig stopped"
        ;;
    *) echo "expected start or stop" >&2; exit 2 ;;
esac
