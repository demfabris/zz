#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

work="$HOME/if-shell-background-order-work-$side"
transcript="$HOME/if-shell-background-order.txt"
rm -rf "$work"
mkdir -p "$work"
: >"$transcript"

wait_for_order() {
    attempt=0
    while [ "$attempt" -lt 300 ]; do
        value="$(main_client display-message -p '#{@order}')"
        if [ "${#value}" -ge 6 ]; then
            printf '%s' "$value"
            return
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    printf 'timeout:%s' "$value"
}

round() {
    label=$1
    first_condition=$2
    config="$work/$label.conf"
    {
        echo "set -g @order ''"
        echo "if -b $first_condition 'set -ag @order 1'"
        for branch in 2 3 4 5 6; do
            echo "if -b true 'set -ag @order $branch'"
        done
    } >"$config"
    main_client set -g @order '' >/dev/null
    main_client source-file "$config"
    printf '%s=%s\n' "$label" "$(wait_for_order)" >>"$transcript"
}

round equal-cost-a true
round equal-cost-b true
round equal-cost-c true
round slow-first "'sleep 0.3'"

main_client set -gu @order >/dev/null 2>&1 || true
rm -rf "$work"
