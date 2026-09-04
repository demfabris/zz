#[cfg(not(target_family = "wasm"))]
fn main() {
    zz_ui_showcase::run_native();
}

#[cfg(target_family = "wasm")]
fn main() {}
