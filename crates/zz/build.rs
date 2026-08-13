use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
    stamp_gpui_revision();
}

/// Stamp the gpui revision this build links, read from the lock file. A
/// hand-written constant drifts the first time the `[patch]` rev moves.
fn stamp_gpui_revision() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let lock = manifest.join("../../Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let revision = std::fs::read_to_string(&lock)
        .ok()
        .and_then(|lock| locked_source(&lock, "gpui"))
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=ZZ_GPUI_SOURCE={revision}");
}

/// The `source` of one package in a Cargo lock file, verbatim.
fn locked_source(lock: &str, package: &str) -> Option<String> {
    let entry = lock
        .split("[[package]]")
        .find(|entry| entry.contains(&format!("\nname = \"{package}\"\n")))?;
    let source = entry
        .lines()
        .find_map(|line| line.strip_prefix("source = \""))?;
    Some(source.trim_end_matches('"').to_owned())
}
