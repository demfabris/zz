use std::{env, fs, io, path::PathBuf};

use thiserror::Error;
use zz_protocol::{
    BrowserProfileNameError, DEFAULT_BROWSER_PROFILE, normalize_browser_profile_name,
};

const NAMED_PROFILE_PREFIX: &str = "zz-profile-";
const EGRESS_PROFILE_MARKER: &str = "@egress-";
const EGRESS_HOST_HASH_HEX_BYTES: usize = 8;
const FNV1A_64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_64_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Error)]
pub enum BrowserProfileError {
    #[error(transparent)]
    Name(#[from] BrowserProfileNameError),
    #[error("invalid client-local egress browser profile")]
    InvalidEgressProfile,
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserProfilePaths {
    pub root: PathBuf,
    pub profile: PathBuf,
}

impl BrowserProfilePaths {
    pub fn egress_profile_name(
        profile: &str,
        egress_host: &str,
    ) -> Result<String, BrowserProfileNameError> {
        egress_profile_name(profile, egress_host)
    }

    /// Create and restrict the CEF root and persistent profile directories.
    pub fn ensure(&self) -> Result<(), BrowserProfileError> {
        self.ensure_profile(DEFAULT_BROWSER_PROFILE)?;
        Ok(())
    }

    /// Resolve, create, and restrict a named zz-owned CEF profile. Every profile
    /// directory stays an immediate child of the CEF root.
    pub fn ensure_profile(&self, name: &str) -> Result<(String, PathBuf), BrowserProfileError> {
        let name = normalize_browser_profile_name(name)?;
        let profile = self.profile_path_for_normalized(&name);
        fs::create_dir_all(&profile)?;
        restrict_to_current_user(&self.root)?;
        restrict_to_current_user(&profile)?;
        Ok((name, profile))
    }

    /// Resolve a profile path without creating it.
    pub fn profile_path(&self, name: &str) -> Result<PathBuf, BrowserProfileNameError> {
        let name = normalize_browser_profile_name(name)?;
        Ok(self.profile_path_for_normalized(&name))
    }

    pub(crate) fn ensure_egress_profile(&self, name: &str) -> Result<PathBuf, BrowserProfileError> {
        if !is_egress_profile_name(name) {
            return Err(BrowserProfileError::InvalidEgressProfile);
        }
        let profile = self.profile_path_for_normalized(name);
        fs::create_dir_all(&profile)?;
        restrict_to_current_user(&self.root)?;
        restrict_to_current_user(&profile)?;
        Ok(profile)
    }

    fn profile_path_for_normalized(&self, name: &str) -> PathBuf {
        if name == DEFAULT_BROWSER_PROFILE {
            return self.profile.clone();
        }
        self.root.join(format!(
            "{NAMED_PROFILE_PREFIX}{}",
            hex_encode(name.as_bytes())
        ))
    }
}

fn egress_profile_name(
    profile: &str,
    egress_host: &str,
) -> Result<String, BrowserProfileNameError> {
    let profile = normalize_browser_profile_name(profile)?;
    let hash = egress_host
        .bytes()
        .fold(FNV1A_64_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(FNV1A_64_PRIME)
        });
    let hash = format!("{hash:016x}");
    Ok(format!(
        "{profile}{EGRESS_PROFILE_MARKER}{}",
        &hash[..EGRESS_HOST_HASH_HEX_BYTES]
    ))
}

fn is_egress_profile_name(name: &str) -> bool {
    let Some((profile, hash)) = name.rsplit_once(EGRESS_PROFILE_MARKER) else {
        return false;
    };
    normalize_browser_profile_name(profile).is_ok_and(|normalized| normalized == profile)
        && hash.len() == EGRESS_HOST_HASH_HEX_BYTES
        && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

/// Resolve the private zz CEF root and its immediate-child profile path.
pub fn resolve_profile_paths() -> io::Result<BrowserProfilePaths> {
    let root = browser_data_dir()?.join("root");
    Ok(BrowserProfilePaths {
        // CEF only accepts persistent profiles that are immediate children of
        // root_cache_path.
        profile: root.join("zz-default"),
        root,
    })
}

/// The file storing the recently visited page list, beside the CEF root.
pub fn recent_pages_path() -> io::Result<PathBuf> {
    Ok(browser_data_dir()?.join("recent-pages"))
}

fn browser_data_dir() -> io::Result<PathBuf> {
    let data = platform_data_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not resolve the current user's application-data directory",
        )
    })?;
    Ok(data.join("zz").join("browser"))
}

#[cfg(target_os = "linux")]
fn platform_data_dir() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| {
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".local").join("share"))
    })
}

#[cfg(target_os = "macos")]
fn platform_data_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Library").join("Application Support"))
}

#[cfg(target_os = "windows")]
fn platform_data_dir() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_data_dir() -> Option<PathBuf> {
    None
}

#[cfg(unix)]
fn restrict_to_current_user(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_to_current_user(_: &std::path::Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_is_an_immediate_child_of_the_private_root() {
        let root = PathBuf::from("app-data/zz/browser/root");
        let paths = BrowserProfilePaths {
            profile: root.join("zz-default"),
            root: root.clone(),
        };
        assert_eq!(paths.profile.parent(), Some(root.as_path()));
        let named = paths
            .profile_path("../Work/Profile")
            .expect("encoded profile name");
        assert_eq!(named.parent(), Some(root.as_path()));
        assert_eq!(
            named.file_name().and_then(|name| name.to_str()),
            Some("zz-profile-2e2e2f576f726b2f50726f66696c65")
        );
    }

    #[test]
    fn legacy_default_alias_reuses_the_existing_profile() {
        let root = PathBuf::from("app-data/zz/browser/root");
        let paths = BrowserProfilePaths {
            profile: root.join("zz-default"),
            root,
        };
        assert_eq!(
            paths.profile_path("zz-default").expect("default alias"),
            paths.profile
        );
    }

    #[test]
    fn egress_profiles_are_stable_flat_and_isolated_by_host() {
        let root = PathBuf::from("app-data/zz/browser/root");
        let paths = BrowserProfilePaths {
            profile: root.join("zz-default"),
            root: root.clone(),
        };
        let first = egress_profile_name("Work", "build.internal").expect("first composite");
        let same = egress_profile_name("Work", "build.internal").expect("stable composite");
        let other = egress_profile_name("Work", "db.internal").expect("other composite");

        assert_eq!(first, same);
        assert_ne!(first, other);
        assert_ne!(first, "Work");
        assert!(is_egress_profile_name(&first));

        let composite_path = paths.profile_path_for_normalized(&first);
        let plain_path = paths.profile_path("Work").expect("plain profile path");
        assert_eq!(composite_path.parent(), Some(root.as_path()));
        assert_ne!(composite_path, plain_path);
    }

    #[test]
    fn egress_profiles_bound_long_hosts_and_accept_maximum_daemon_profiles() {
        let profile = "p".repeat(zz_protocol::MAX_BROWSER_PROFILE_NAME_BYTES);
        let short = egress_profile_name(&profile, "host").expect("short-host composite");
        let long = egress_profile_name(&profile, &"h".repeat(4096)).expect("long-host composite");

        assert_eq!(
            short.len(),
            zz_protocol::MAX_BROWSER_PROFILE_NAME_BYTES
                + EGRESS_PROFILE_MARKER.len()
                + EGRESS_HOST_HASH_HEX_BYTES
        );
        assert_eq!(short.len(), long.len());
        assert!(is_egress_profile_name(&short));
        assert!(is_egress_profile_name(&long));
    }

    #[cfg(unix)]
    #[test]
    fn ensures_user_only_unix_permissions() {
        use std::{
            os::unix::fs::PermissionsExt,
            time::{SystemTime, UNIX_EPOCH},
        };

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time is after the Unix epoch")
            .as_nanos();
        let test_root = env::temp_dir().join(format!("zz-profile-{}-{unique}", std::process::id()));
        let paths = BrowserProfilePaths {
            profile: test_root.join("zz-default"),
            root: test_root.clone(),
        };
        paths.ensure().expect("create test browser profile");

        let root_mode = fs::metadata(&paths.root)
            .expect("read root metadata")
            .permissions()
            .mode()
            & 0o777;
        let profile_mode = fs::metadata(&paths.profile)
            .expect("read profile metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(root_mode, 0o700);
        assert_eq!(profile_mode, 0o700);
        fs::remove_dir_all(test_root).expect("remove test browser profile");
    }
}
