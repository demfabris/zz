#!/usr/bin/env bash
if [ "${ZZ_ALIAS_CARGO_SCOPED:-0}" != 1 ]; then
  export ZZ_ALIAS_CARGO_SCOPED=1
  exec /tmp/zz-cargo.sh "$@"
fi
args=("$@")
n=${#args[@]}
if [ "$n" -ge 2 ] && [ "${args[n-2]}" = --jobs ]; then
  jobs="${args[n-1]}"
  unset 'args[n-1]' 'args[n-2]'
  if [ "${args[0]}" != fmt ]; then
    args=("${args[0]}" --jobs "$jobs" "${args[@]:1}")
  fi
fi
exec /home/demfabris/.cargo/bin/cargo "${args[@]}"
