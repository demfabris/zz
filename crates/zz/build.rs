fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(target_os = "linux")]
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN");
}
