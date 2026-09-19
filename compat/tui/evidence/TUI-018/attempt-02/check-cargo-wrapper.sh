#!/usr/bin/env bash
before=()
after=()
separator=0
for argument in "$@"; do
  if [ "$argument" = -- ]; then
    separator=1
  elif [ "$separator" -eq 0 ]; then
    before+=("$argument")
  else
    after+=("$argument")
  fi
done
S=$((RANDOM % 2))
exec systemd-run --user --scope -q -p MemoryMax=8G -p MemorySwapMax=4G flock -w 540 /tmp/zz-cargo-slot-$S.lock /home/demfabris/.cargo/bin/cargo "${before[@]}" --jobs 3 -- "${after[@]}" --test-threads=3
