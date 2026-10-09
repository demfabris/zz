//! Location and permission policy for user-owned application data.

use std::{fs, io, path::Path};

pub use zz_protocol::app_identity::platform_data_dir;

#[cfg(unix)]
pub fn restrict_to_current_user(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
pub fn restrict_to_current_user(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub fn restrict_directory_to_current_user(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
pub fn restrict_directory_to_current_user(_: &Path) -> io::Result<()> {
    Ok(())
}
