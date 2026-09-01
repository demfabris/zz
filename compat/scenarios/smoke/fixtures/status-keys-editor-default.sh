#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    probe_bin() { printf '%s' "$ZZ_SMOKE_ZZ_BIN"; }
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    side=pin
    probe_bin() { printf '%s' "$ZZ_SMOKE_TMUX_BIN"; }
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

socket="/tmp/zz-status-keys-$side-$$.sock"
bin="$(probe_bin)"

probe() {
    label="$1"
    shift
    rm -f "$socket"
    env -u TMUX -u TMUX_PANE -u VISUAL -u EDITOR "$@" \
        "$bin" -f /dev/null -S "$socket" new-session -d -s probe >/dev/null 2>&1 || true
    status="$("$bin" -S "$socket" show-options -gv status-keys 2>&1 || true)"
    mode="$("$bin" -S "$socket" show-options -gwv mode-keys 2>&1 || true)"
    editor="$("$bin" -S "$socket" show-options -gv editor 2>&1 || true)"
    "$bin" -S "$socket" kill-server >/dev/null 2>&1 || true
    printf '%s=%s/%s/%s' "$label" "$status" "$mode" "$editor"
}

matrix="$(probe scrubbed)"
matrix="$matrix $(probe editor-vi EDITOR=vi)"
matrix="$matrix $(probe editor-path-nvim EDITOR=/usr/local/bin/nvim)"
matrix="$matrix $(probe visual-wins VISUAL=vim EDITOR=emacs)"
matrix="$matrix $(probe editor-emacs EDITOR=emacs)"
matrix="$matrix $(probe visual-empty VISUAL= EDITOR=vim)"
matrix="$matrix $(probe directory-only-vi EDITOR=/opt/vi/bin/ed)"
matrix="$matrix $(probe substring-vi EDITOR=nano-vi-x)"

main_client set-environment -g STATUS_KEYS_MATRIX "$matrix"
