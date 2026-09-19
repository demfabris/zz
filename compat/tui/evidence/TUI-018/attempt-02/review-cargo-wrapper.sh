#!/bin/bash
export CARGO_HOME=/home/demfabris/.cargo RUSTUP_HOME=/home/demfabris/.rustup
args=("$@")
if [[ "$1" == test && " $* " != *' --jobs '* ]]; then
  args=()
  added=0
  for arg in "$@"; do
    if [[ "$arg" == -- ]]; then
      args+=(--jobs 4 -- --test-threads=4)
      added=1
    else
      args+=("$arg")
    fi
  done
  if (( added == 0 )); then args+=(--jobs 4 -- --test-threads=4); fi
fi
S=$((RANDOM % 2))
exec systemd-run --user --scope -q -p MemoryMax=10G -p MemorySwapMax=4G flock -w 540 /tmp/zz-cargo-slot-$S.lock /home/demfabris/.cargo/bin/cargo "${args[@]}"
