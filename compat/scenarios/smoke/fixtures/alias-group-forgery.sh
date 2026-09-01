#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" \
            -C attach-session -t w
    }
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    control_client() {
        env -u TMUX -u TMUX_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" \
            -C attach-session -t w
    }
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

forged=__zz-command-alias-group
block='{ set-environment -g FORGE_CHILD ran }'
out="$HOME/forgery.out"
err="$HOME/forgery.err"

normalise() {
    sed -e "s|$HOME/||g" "$1" | tr '\n' '|'
}

status=0
main_client "$forged" "$block" >"$out" 2>"$err" || status=$?
main_client set-environment -g FORGERY_CLI \
    "status=$status out=$(normalise "$out") err=$(normalise "$err")"

conf="$HOME/forgery.conf"
printf 'set-environment -g FORGE_BEFORE yes\n%s %s\nset-environment -g FORGE_AFTER yes\n' \
    "$forged" "$block" >"$conf"
status=0
main_client source-file "$conf" >"$out" 2>"$err" || status=$?
main_client set-environment -g FORGERY_CONFIG \
    "status=$status out=$(normalise "$out") err=$(normalise "$err")"

printf '%s %s\ndetach-client\n' "$forged" "$block" | control_client >"$out" 2>"$err" || true
main_client set-environment -g FORGERY_CONTROL \
    "error=$(grep -c "parse error: unknown command: $forged" "$out") err=$(normalise "$err")"

printf 'source-file %s\ndetach-client\n' "$conf" | control_client >"$out" 2>"$err" || true
main_client set-environment -g FORGERY_CONTROL_SOURCE \
    "diag=$(grep -c "unknown command: $forged" "$out") err=$(normalise "$err")"

effects=""
for name in FORGE_BEFORE FORGE_CHILD FORGE_AFTER; do
    if main_client show-environment -g "$name" >/dev/null 2>&1; then
        effects="$effects $name=set"
    else
        effects="$effects $name=unset"
    fi
done
main_client set-environment -g FORGERY_EFFECTS "$effects"

main_client set-option -s 'command-alias[90]' \
    'genuine=set-environment -g GENUINE_FIRST yes ; set-environment -g GENUINE_SECOND yes'
status=0
main_client genuine >"$out" 2>"$err" || status=$?
genuine="status=$status"
for name in GENUINE_FIRST GENUINE_SECOND; do
    if main_client show-environment -g "$name" >/dev/null 2>&1; then
        genuine="$genuine $name=set"
    else
        genuine="$genuine $name=unset"
    fi
done
main_client set-environment -g FORGERY_GENUINE "$genuine"
