#!/usr/bin/env bash
set -euo pipefail

gtk_repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
gtk_target="${ZZ_GTK_TARGET_DIR:-$gtk_repo/crates/zz-gtk/target}"
gtk_daemon_target="${CARGO_TARGET_DIR:-$gtk_repo/target}"

if [[ "$(uname -s)" != Linux ]]; then
    echo "The GTK development launcher requires Linux." >&2
    exit 2
fi

cd "$gtk_repo"
cargo build --locked -p zz --bin zz --target-dir "$gtk_daemon_target"
gtk_daemon_bundle="$(cd "$gtk_daemon_target/debug" && pwd)"
cargo build --locked --manifest-path crates/zz-gtk/Cargo.toml --target-dir "$gtk_target" --bin zz-gtk
gtk_bundle="$(cd "$gtk_target/debug" && pwd)"

for resource in libcef.so icudtl.dat resources.pak locales/en-US.pak; do
    if [[ ! -s "$gtk_bundle/$resource" ]]; then
        echo "CEF did not stage $resource beside zz-gtk in $gtk_bundle." >&2
        echo "Run cargo clean --manifest-path crates/zz-gtk/Cargo.toml --target-dir '$gtk_target' -p cef-dll-sys, then retry just gtk." >&2
        exit 1
    fi
done

exec env ZZ_GTK_DAEMON_EXECUTABLE="$gtk_daemon_bundle/zz" LD_LIBRARY_PATH="$gtk_bundle${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$gtk_bundle/zz-gtk" "$@"
