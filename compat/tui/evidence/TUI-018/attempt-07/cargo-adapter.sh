#!/usr/bin/env bash
set -euo pipefail
if [ "${ZZ_ALIAS_CARGO_WRAPPED:-0}" != 1 ]; then
  export ZZ_ALIAS_CARGO_WRAPPED=1
  exec /tmp/zz-cargo.sh "$@"
fi
args=("$@")
count=${#args[@]}
jobs=${args[count-1]}
unset 'args[count-1]' 'args[count-2]'
if [ "${args[0]}" != fmt ]; then
  args=("${args[0]}" --jobs "$jobs" "${args[@]:1}")
fi
exec /home/demfabris/.cargo/bin/cargo "${args[@]}"
