#!/usr/bin/env bash
E=compat/tui/evidence/TUI-018/attempt-04
bash "$E/run.sh" test-protocol /tmp/zz-cargo.sh test -p zz-protocol
bash "$E/run.sh" compat-check compat/check.sh
bash "$E/run.sh" clippy /tmp/zz-cargo.sh clippy -p zz-daemon -p zz-mux -p zz-protocol -p zz --all-targets --all-features -- -D warnings
bash "$E/run.sh" fmt-final /tmp/zz-cargo.sh fmt --all
