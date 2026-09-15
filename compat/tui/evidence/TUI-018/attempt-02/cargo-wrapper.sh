#!/usr/bin/env bash
S=$((RANDOM % 2))
exec systemd-run --user --scope -q -p MemoryMax=8G -p MemorySwapMax=4G flock -w 540 /tmp/zz-cargo-slot-$S.lock cargo "$@"
