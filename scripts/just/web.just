import "settings.just"

set working-directory := "../.."

# Install the toolchain used by the browser client.
setup:
    rustup toolchain install nightly --profile minimal --component rustfmt --component clippy
    rustup target add --toolchain nightly wasm32-unknown-unknown
    command -v cargo-watch >/dev/null 2>&1 || cargo install cargo-watch --locked
    cargo install wasm-bindgen-cli --version 0.2.128 --locked

run:
    @scripts/web-dev.sh

build *args:
    @scripts/build-web-wasm.sh {{ args }}

serve *args:
    @scripts/web-dev.sh --serve-only {{ args }}
