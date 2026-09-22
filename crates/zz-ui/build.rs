use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let lock = manifest.join("../../Cargo.lock");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", lock.display());
    let revision = std::fs::read_to_string(&lock)
        .ok()
        .and_then(|lock| {
            let entry = lock
                .split("[[package]]")
                .find(|entry| entry.contains("\nname = \"gpui\"\n"))?
                .to_owned();
            let source = entry
                .lines()
                .find_map(|line| line.strip_prefix("source = \""))?
                .trim_end_matches('"')
                .to_owned();
            let revision = source
                .split_once('#')
                .map_or(source.as_str(), |(_, rev)| rev);
            Some(revision.get(..8).unwrap_or(revision).to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=ZZ_UI_GPUI_REVISION={revision}");
}
