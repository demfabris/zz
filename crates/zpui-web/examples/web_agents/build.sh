#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
OUT="$ROOT/target/web-agents"
PROFILE=debug
ARGS=()
if [[ "${1:-}" == "--release" ]]; then
    PROFILE=release
    ARGS=(--release)
fi

cd "$ROOT"
rustup run nightly cargo build --locked -p zpui-web --no-default-features --example web_agents \
    --target wasm32-unknown-unknown --target-dir "$ROOT/target/web-agents-build" ${ARGS[@]+"${ARGS[@]}"}
mkdir -p "$OUT/wasm"
wasm-bindgen "$ROOT/target/web-agents-build/wasm32-unknown-unknown/$PROFILE/examples/web_agents.wasm" \
    --out-dir "$OUT/wasm" --out-name web_agents --target web --no-typescript
cp "$ROOT/crates/zpui-web/examples/web_agents/index.html" "$OUT/index.html"
echo "built $OUT; serve it with: python3 -m http.server -d $OUT 8095"
