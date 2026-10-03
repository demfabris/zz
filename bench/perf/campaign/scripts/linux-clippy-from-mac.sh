#!/usr/bin/env bash
set -eu
if [ $# -lt 2 ]; then echo "usage: $0 <worktree> <crate>... (LXTARGET=<target dir>)" >&2; exit 2; fi
wt=$1; shift
cc=${TMPDIR:-/tmp}/zz-lxcc; mkdir -p "$cc"
for tool in gcc g++; do
  driver=cc; [ "$tool" = g++ ] && driver=c++
  printf '#!/bin/bash\nargs=()\nfor a in "$@"; do case "$a" in --target=*) ;; *) args+=("$a");; esac; done\nexec zig %s -target x86_64-linux-gnu.2.35 "${args[@]}"\n' "$driver" > "$cc/x86_64-linux-gnu-$tool"
done
printf '#!/bin/bash\nexec zig ar "$@"\n' > "$cc/x86_64-linux-gnu-ar"
chmod +x "$cc"/x86_64-linux-gnu-*
pk=(); for c in "$@"; do pk+=(-p "$c"); done
cd "$wt"
unset GHOSTTY_SOURCE_DIR
export CC_x86_64_unknown_linux_gnu=$cc/x86_64-linux-gnu-gcc CXX_x86_64_unknown_linux_gnu=$cc/x86_64-linux-gnu-g++ AR_x86_64_unknown_linux_gnu=$cc/x86_64-linux-gnu-ar
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=$cc/x86_64-linux-gnu-gcc RUST_FONTCONFIG_DLOPEN=1 FREETYPE2_NO_PKG_CONFIG=1 PKG_CONFIG_ALLOW_CROSS=1
export CARGO_TARGET_DIR=${LXTARGET:-$wt/target/lx}
exec cargo clippy --target x86_64-unknown-linux-gnu "${pk[@]}" --all-targets --all-features -- -D warnings
