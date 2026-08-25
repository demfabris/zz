#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

parse_out="$HOME/cli-chain-parse-abort.out"
parse_err="$HOME/cli-chain-parse-abort.err"
parse_expected="$HOME/cli-chain-parse-abort.expected"
marker_expected="$HOME/cli-chain-marker.expected"
printf '%s\n' 'unknown command: frobnicate' >"$parse_expected"
printf '%s\n' 'unknown variable: CLI_CHAIN_BEFORE' >"$marker_expected"
set +e
main_client set-environment -g CLI_CHAIN_BEFORE mutated ';' frobnicate >"$parse_out" 2>"$parse_err"
parse_status=$?
main_client show-environment -g CLI_CHAIN_BEFORE >"$parse_out.marker" 2>"$parse_err.marker"
marker_status=$?
set -e

if [ "$parse_status" -eq 1 ] && [ ! -s "$parse_out" ] && \
   cmp -s "$parse_expected" "$parse_err" && \
   [ "$marker_status" -eq 1 ] && [ ! -s "$parse_out.marker" ] && \
   cmp -s "$marker_expected" "$parse_err.marker"; then
    main_client set-environment -g CLI_PARSE_ABORT clean
else
    main_client set-environment -g CLI_PARSE_ABORT broken
fi

runtime_out="$HOME/cli-chain-runtime.out"
runtime_err="$HOME/cli-chain-runtime.err"
runtime_expected="$HOME/cli-chain-runtime.expected"
before_expected="$HOME/cli-chain-runtime-before.expected"
after_expected="$HOME/cli-chain-runtime-after.expected"
printf '%s\n' "can't find session: missing" >"$runtime_expected"
printf '%s\n' 'CLI_RUNTIME_BEFORE=kept' >"$before_expected"
printf '%s\n' 'unknown variable: CLI_RUNTIME_AFTER' >"$after_expected"
set +e
main_client set-environment -g CLI_RUNTIME_BEFORE kept ';' has-session -t missing ';' set-environment -g CLI_RUNTIME_AFTER bad >"$runtime_out" 2>"$runtime_err"
runtime_status=$?
main_client show-environment -g CLI_RUNTIME_BEFORE >"$runtime_out.before" 2>"$runtime_err.before"
before_status=$?
main_client show-environment -g CLI_RUNTIME_AFTER >"$runtime_out.after" 2>"$runtime_err.after"
after_status=$?
set -e

if [ "$runtime_status" -eq 1 ] && [ ! -s "$runtime_out" ] && \
   cmp -s "$runtime_expected" "$runtime_err" && \
   [ "$before_status" -eq 0 ] && \
   cmp -s "$before_expected" "$runtime_out.before" && \
   [ ! -s "$runtime_err.before" ] && \
   [ "$after_status" -eq 1 ] && [ ! -s "$runtime_out.after" ] && \
   cmp -s "$after_expected" "$runtime_err.after"; then
    main_client set-environment -g CLI_RUNTIME_ORDER clean
else
    main_client set-environment -g CLI_RUNTIME_ORDER broken
fi
