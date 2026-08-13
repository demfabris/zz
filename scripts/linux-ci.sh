#!/usr/bin/env bash
# Run pieces of the Linux CI leg in a local container instead of burning a
# 30-minute Actions round trip: clippy findings and compile errors that live
# in linux-only cfg branches never surface in a macOS cargo run.
#
# With no arguments this mirrors the Lint steps (fmt --check, then clippy with
# -D warnings); any arguments run verbatim in the container instead:
#     scripts/linux-ci.sh cargo test -p zz-daemon --lib
#     scripts/linux-ci.sh bash
# The image is built on first use (REBUILD=1 forces it). The cargo registry,
# target dir, and CEF distribution persist in the zz-linux-ci-state volume, so
# the first run downloads a lot and later runs are incremental.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE=zz-linux-ci
ZIG_VERSION=0.16.0

command -v docker >/dev/null 2>&1 || { echo "error: docker is required" >&2; exit 1; }

if [[ "${REBUILD:-0}" != 0 ]] || ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    docker build -t "$IMAGE" --build-arg ZIG_VERSION="$ZIG_VERSION" - <<'DOCKERFILE'
FROM rust:1.97-bookworm
# The second line carries libcef.so's link/runtime closure (the GitHub
# runner image ships these desktop libraries out of the box).
RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake curl desktop-file-utils ninja-build lld pkg-config xz-utils \
        libfontconfig-dev libfreetype-dev libwayland-dev libx11-dev \
        libx11-xcb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
        libnss3 libcups2 libdbus-1-3 libatk1.0-0 libatk-bridge2.0-0 \
        libgbm1 libasound2 libxcomposite1 libxdamage1 libxfixes3 \
        libxrandr2 libpango-1.0-0 libcairo2 libexpat1 libxkbcommon-x11-0 \
    && rm -rf /var/lib/apt/lists/*
ARG ZIG_VERSION
# Release archives renamed from zig-linux-<arch> to zig-<arch>-linux along the
# way, so try both.
RUN arch="$(uname -m)" \
    && for name in "zig-$arch-linux-$ZIG_VERSION" "zig-linux-$arch-$ZIG_VERSION"; do \
        curl -fsSL "https://ziglang.org/download/$ZIG_VERSION/$name.tar.xz" \
            -o /tmp/zig.tar.xz && break; \
    done \
    && mkdir -p /opt/zig \
    && tar -C /opt/zig --strip-components=1 -xf /tmp/zig.tar.xz \
    && rm /tmp/zig.tar.xz \
    && ln -s /opt/zig/zig /usr/local/bin/zig \
    && zig version
RUN rustup component add clippy rustfmt
DOCKERFILE
fi

if [[ $# -eq 0 ]]; then
    set -- bash -c 'cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings'
fi

tty_flags=()
if [[ -t 0 && -t 1 ]]; then
    tty_flags=(-it)
fi

exec docker run --rm ${tty_flags[@]+"${tty_flags[@]}"} \
    -v "$REPO_ROOT":/w \
    -v zz-linux-ci-state:/state \
    -e CARGO_HOME=/state/cargo \
    -e CARGO_TARGET_DIR=/state/target \
    -e CEF_PATH=/state/cef \
    -w /w \
    "$IMAGE" "$@"
