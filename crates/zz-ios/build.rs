use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    embed_simulator_entitlements();
}

/// Gives the simulator bundle a keychain. A simulator binary is not signed, so
/// the loader reads entitlements from a `__TEXT,__entitlements` section, and
/// without it every `SecItem` call answers `errSecMissingEntitlement`.
fn embed_simulator_entitlements() {
    if std::env::var("TARGET").as_deref() != Ok("aarch64-apple-ios-sim") {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let entitlements = manifest.join("ios/sim.entitlements");
    println!("cargo:rerun-if-changed={}", entitlements.display());
    println!(
        "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__entitlements,{}",
        entitlements.display(),
    );
}
