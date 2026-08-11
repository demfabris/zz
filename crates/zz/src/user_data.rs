//! Location and permission policy for user-owned application data.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

#[cfg(target_os = "linux")]
pub(crate) fn platform_data_dir() -> Option<PathBuf> {
    absolute_env_path("XDG_DATA_HOME")
        .or_else(|| absolute_env_path("HOME").map(|home| home.join(".local").join("share")))
}

/// iOS keeps the same layout, inside the app container `HOME` points at.
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn platform_data_dir() -> Option<PathBuf> {
    absolute_env_path("HOME").map(|home| home.join("Library").join("Application Support"))
}

#[cfg(target_os = "windows")]
pub(crate) fn platform_data_dir() -> Option<PathBuf> {
    absolute_env_path("LOCALAPPDATA")
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
)))]
pub(crate) fn platform_data_dir() -> Option<PathBuf> {
    None
}

#[cfg(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
))]
#[cfg_attr(target_os = "ios", allow(dead_code))]
fn absolute_env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(unix)]
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn restrict_to_current_user(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
pub(crate) fn restrict_to_current_user(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
#[cfg_attr(target_os = "ios", allow(dead_code))]
pub(crate) fn restrict_directory_to_current_user(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
pub(crate) fn restrict_directory_to_current_user(_: &Path) -> io::Result<()> {
    Ok(())
}
