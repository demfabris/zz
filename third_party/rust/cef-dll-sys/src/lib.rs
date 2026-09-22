pub use upstream::*;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[allow(non_snake_case, clippy::missing_safety_doc)]
mod dynamic;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
include!("exports.rs");

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn get_cef_dir() -> Option<std::path::PathBuf> {
    let configured = std::env::var_os("FLATPAK")
        .map(|_| std::path::PathBuf::from("/usr/lib"))
        .or_else(|| std::env::var_os("CEF_PATH").map(std::path::PathBuf::from));
    let directory = match configured {
        Some(path) => [
            path.join(
                env!("CARGO_PKG_VERSION")
                    .split_once('+')
                    .map_or(env!("CARGO_PKG_VERSION"), |(_, version)| version),
            )
            .join("cef_linux_x86_64"),
            path,
        ]
        .into_iter()
        .find(|path| path.is_dir())?,
        None => std::path::PathBuf::from(option_env!("ZZ_CEF_DISTRIBUTION_DIR")?),
    };
    directory.canonicalize().ok()
}
