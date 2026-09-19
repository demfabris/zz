#!/usr/bin/env bash
E=compat/tui/evidence/TUI-018/attempt-03
bash "$E/run.sh" test-stream-closing /tmp/zz-cargo.sh test -p zz --test cli_binary caller_stream_ -- --test-threads=1
bash "$E/run.sh" build-closing /tmp/zz-cargo.sh build -p zz
bash "$E/run.sh" test-palette-solo /tmp/zz-cargo.sh test -p zz --lib tab_accepts_completion_without_leaving_the_palette -- --exact command::palette::tests::tab_accepts_completion_without_leaving_the_palette
bash "$E/run.sh" test-settings-solo /tmp/zz-cargo.sh test -p zz --lib settings_view_is_lazy_and_retained -- --exact workspace::sidebar::tests::settings_view_is_lazy_and_retained
bash "$E/run.sh" clippy-closing /tmp/zz-cargo.sh clippy -p zz-daemon -p zz-mux -p zz-protocol -p zz --all-targets --all-features -- -D warnings
bash "$E/run.sh" fmt-closing /tmp/zz-cargo.sh fmt --all
