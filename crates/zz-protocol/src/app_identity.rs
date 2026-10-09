use std::{env, io, path::PathBuf};

pub const DEVELOPMENT: bool = match option_env!("ZZ_DEV_BUILD") {
    Some(value) => matches!(value.as_bytes(), b"1"),
    None => false,
};

pub const DIRECTORY: &str = if DEVELOPMENT { "zz-dev" } else { "zz" };
pub const DISPLAY_NAME: &str = if DEVELOPMENT { "zz Dev" } else { "zz" };
pub const MACOS_BUNDLE_ID: &str = if DEVELOPMENT {
    "dev.zz.app.dev"
} else {
    "dev.zz.app"
};

/// The file storing the browser's recently visited page list, beside its CEF root.
pub fn recent_pages_path() -> io::Result<PathBuf> {
    Ok(browser_data_dir()?.join("recent-pages"))
}

pub fn browser_data_dir() -> io::Result<PathBuf> {
    let data = platform_data_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not resolve the current user's application-data directory",
        )
    })?;
    Ok(data.join(DIRECTORY).join("browser"))
}

#[cfg(target_os = "linux")]
pub fn platform_data_dir() -> Option<PathBuf> {
    absolute_env_path("XDG_DATA_HOME")
        .or_else(|| absolute_env_path("HOME").map(|home| home.join(".local").join("share")))
}

/// iOS keeps the same layout, inside the app container `HOME` points at.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub fn platform_data_dir() -> Option<PathBuf> {
    absolute_env_path("HOME").map(|home| home.join("Library").join("Application Support"))
}

#[cfg(target_os = "windows")]
pub fn platform_data_dir() -> Option<PathBuf> {
    absolute_env_path("LOCALAPPDATA")
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
)))]
pub fn platform_data_dir() -> Option<PathBuf> {
    None
}

#[cfg(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "ios",
    target_os = "windows"
))]
fn absolute_env_path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}
