#!/bin/bash
for arg in "$@"; do
  if [[ "$arg" == zzcs-twostream ]]; then
    exec /home/demfabris/dev/zz-box-reds/target/debug/zz "$@" </dev/null
  fi
done
exec /home/demfabris/dev/zz-box-reds/target/debug/zz "$@"
