use std::{
    collections::HashSet,
    fmt,
    fs::File,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zz_browser::normalize_browser_profile_name;

use crate::fs_util::{atomic_write, restrict_to_current_user};

pub(crate) const MAX_LOCAL_STATE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PROFILE_CACHE_BYTES: u64 = 128 * 1024;
const MAX_DISCOVERED_PROFILES: usize = 64;
const MAX_LABEL_BYTES: usize = 256;
const ZZ_PROFILE_PREFIX: &str = "chrome:";
const PROFILE_CACHE_VERSION: u8 = 1;
const PROFILE_CACHE_FILE_NAME: &str = "chrome-profile-metadata.json";

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DetectedChromeProfile {
    pub zz_profile: String,
    pub display_name: String,
    pub email: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CachedChromeProfiles {
    version: u8,
    profiles: Vec<DetectedChromeProfile>,
}

impl DetectedChromeProfile {
    pub fn menu_label(&self) -> String {
        match self.email.as_deref() {
            Some(email) if self.display_name.eq_ignore_ascii_case(email) => email.to_owned(),
            Some(email) => format!("{} · {email}", self.display_name),
            None => self.display_name.clone(),
        }
    }
}

#[derive(Debug)]
pub enum ChromeProfileDiscoveryError {
    Io(io::Error),
    TooLarge(u64),
    Json(serde_json::Error),
}

impl fmt::Display for ChromeProfileDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error)
                if cfg!(target_os = "macos") && error.kind() == io::ErrorKind::PermissionDenied =>
            {
                formatter.write_str(
                    "macOS blocked access to Chrome profile metadata. Enable zz in System \
                     Settings > Privacy & Security > Full Disk Access, then quit and reopen zz.",
                )
            }
            Self::Io(error) => write!(formatter, "could not read Chrome profile metadata: {error}"),
            Self::TooLarge(bytes) => write!(
                formatter,
                "Chrome profile metadata is too large ({bytes} bytes)"
            ),
            Self::Json(error) => write!(
                formatter,
                "could not parse Chrome profile metadata: {error}"
            ),
        }
    }
}

impl std::error::Error for ChromeProfileDiscoveryError {}

impl From<io::Error> for ChromeProfileDiscoveryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ChromeProfileDiscoveryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Read Google Chrome profile metadata without opening a profile database or
/// credential store.
pub fn discover_profiles() -> Result<Vec<DetectedChromeProfile>, ChromeProfileDiscoveryError> {
    let Some(path) = chrome_local_state_path() else {
        return Ok(Vec::new());
    };
    match discover_profiles_at(&path) {
        Err(ChromeProfileDiscoveryError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Ok(Vec::new())
        }
        result => result,
    }
}

pub fn load_cached_profiles() -> Result<Vec<DetectedChromeProfile>, ChromeProfileDiscoveryError> {
    let path = profile_cache_path()?;
    match load_cached_profiles_at(&path) {
        Err(ChromeProfileDiscoveryError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Ok(Vec::new())
        }
        result => result,
    }
}

pub fn save_cached_profiles(
    profiles: &[DetectedChromeProfile],
) -> Result<(), ChromeProfileDiscoveryError> {
    let path = profile_cache_path()?;
    save_cached_profiles_at(&path, profiles)
}

fn profile_cache_path() -> Result<PathBuf, ChromeProfileDiscoveryError> {
    Ok(zz_browser::recent_pages_path()?.with_file_name(PROFILE_CACHE_FILE_NAME))
}

fn load_cached_profiles_at(
    path: &Path,
) -> Result<Vec<DetectedChromeProfile>, ChromeProfileDiscoveryError> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX_PROFILE_CACHE_BYTES {
        return Err(ChromeProfileDiscoveryError::TooLarge(size));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or_default());
    file.by_ref()
        .take(MAX_PROFILE_CACHE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PROFILE_CACHE_BYTES {
        return Err(ChromeProfileDiscoveryError::TooLarge(
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        ));
    }

    let cache: CachedChromeProfiles = serde_json::from_slice(&bytes)?;
    if cache.version != PROFILE_CACHE_VERSION {
        return Ok(Vec::new());
    }
    Ok(sanitize_cached_profiles(cache.profiles))
}

fn save_cached_profiles_at(
    path: &Path,
    profiles: &[DetectedChromeProfile],
) -> Result<(), ChromeProfileDiscoveryError> {
    let contents = serde_json::to_vec(&CachedChromeProfiles {
        version: PROFILE_CACHE_VERSION,
        profiles: profiles.to_vec(),
    })?;
    atomic_write(path, &contents)?;
    restrict_to_current_user(path)?;
    Ok(())
}

fn sanitize_cached_profiles(profiles: Vec<DetectedChromeProfile>) -> Vec<DetectedChromeProfile> {
    let mut seen = HashSet::new();
    profiles
        .into_iter()
        .filter_map(|profile| {
            let zz_profile = normalize_browser_profile_name(&profile.zz_profile).ok()?;
            if chrome_storage_key(&zz_profile).is_none() || !seen.insert(zz_profile.clone()) {
                return None;
            }
            let display_name = clean_cached_label(&profile.display_name)?;
            let email = profile.email.as_deref().and_then(clean_cached_label);
            Some(DetectedChromeProfile {
                zz_profile,
                display_name,
                email,
            })
        })
        .take(MAX_DISCOVERED_PROFILES)
        .collect()
}

fn clean_cached_label(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= MAX_LABEL_BYTES && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn discover_profiles_at(
    path: &Path,
) -> Result<Vec<DetectedChromeProfile>, ChromeProfileDiscoveryError> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX_LOCAL_STATE_BYTES {
        return Err(ChromeProfileDiscoveryError::TooLarge(size));
    }

    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or_default());
    file.by_ref()
        .take(MAX_LOCAL_STATE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_LOCAL_STATE_BYTES {
        return Err(ChromeProfileDiscoveryError::TooLarge(
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        ));
    }
    parse_profiles(&bytes)
}

fn parse_profiles(bytes: &[u8]) -> Result<Vec<DetectedChromeProfile>, ChromeProfileDiscoveryError> {
    let root: Value = serde_json::from_slice(bytes)?;
    let Some(profile) = root.get("profile") else {
        return Ok(Vec::new());
    };
    let Some(info_cache) = profile.get("info_cache").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };

    let mut seen = HashSet::new();
    let mut ordered_keys = profile
        .get("profiles_order")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|storage_key| info_cache.contains_key(*storage_key) && seen.insert(*storage_key))
        .collect::<Vec<_>>();
    let mut remaining_keys = info_cache
        .keys()
        .map(String::as_str)
        .filter(|storage_key| seen.insert(*storage_key))
        .collect::<Vec<_>>();
    remaining_keys.sort_unstable();
    ordered_keys.extend(remaining_keys);

    let profiles = ordered_keys
        .into_iter()
        .filter_map(|storage_key| profile_from_cache_entry(storage_key, &info_cache[storage_key]))
        .take(MAX_DISCOVERED_PROFILES)
        .collect::<Vec<_>>();
    Ok(profiles)
}

fn profile_from_cache_entry(storage_key: &str, fields: &Value) -> Option<DetectedChromeProfile> {
    let storage_key = clean_storage_key(storage_key)?;
    let fields = fields.as_object()?;
    if fields
        .get("is_ephemeral")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }

    let email = clean_field(fields, "user_name").map(str::to_owned);
    let display_name = clean_field(fields, "name")
        .filter(|name| !name.is_empty())
        .or_else(|| clean_field(fields, "gaia_name").filter(|name| !name.is_empty()))
        .map(str::to_owned)
        .or_else(|| email.clone())
        .unwrap_or_else(|| storage_key.to_owned());
    let zz_profile =
        normalize_browser_profile_name(&format!("{ZZ_PROFILE_PREFIX}{storage_key}")).ok()?;

    Some(DetectedChromeProfile {
        zz_profile,
        display_name,
        email,
    })
}

/// Return the installed Chrome directory name encoded in a zz Chrome profile.
/// The value is safe to append to Chrome's user-data root as one path component.
pub fn chrome_storage_key(zz_profile: &str) -> Option<&str> {
    clean_storage_key(zz_profile.strip_prefix(ZZ_PROFILE_PREFIX)?)
}

fn clean_storage_key(storage_key: &str) -> Option<&str> {
    let storage_key = storage_key.trim();
    (!storage_key.is_empty()
        && storage_key.len()
            <= zz_browser::MAX_BROWSER_PROFILE_NAME_BYTES - ZZ_PROFILE_PREFIX.len()
        && !storage_key.chars().any(char::is_control)
        && !storage_key.contains(['/', '\\'])
        && !matches!(storage_key, "." | ".."))
    .then_some(storage_key)
}

fn clean_field<'a>(fields: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    let value = fields.get(key)?.as_str()?.trim();
    if value.is_empty() || value.len() > MAX_LABEL_BYTES || value.chars().any(char::is_control) {
        return None;
    }
    Some(value)
}

pub(crate) fn chrome_local_state_path() -> Option<PathBuf> {
    chrome_user_data_dir().map(|root| root.join("Local State"))
}

#[cfg(target_os = "macos")]
pub fn chrome_user_data_dir() -> Option<PathBuf> {
    let home = objc2_foundation::NSHomeDirectory().to_string();
    (!home.is_empty())
        .then(|| PathBuf::from(home).join("Library/Application Support/Google/Chrome"))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn chrome_user_data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".config"))
        })
        .map(|root| root.join("google-chrome"))
}

#[cfg(target_os = "windows")]
pub fn chrome_user_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .map(|root| root.join("Google/Chrome/User Data"))
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "windows"
)))]
pub fn chrome_user_data_dir() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_stable_profiles_in_chromes_order() {
        let profiles = parse_profiles(
            br#"{
                "profile": {
                    "profiles_order": ["Default", "Profile 5", "Profile 4", "Profile 3"],
                    "info_cache": {
                        "Default": {
                            "name": "Work",
                            "gaia_id": "gaia-one",
                            "user_name": "dev@example.com"
                        },
                        "Profile 2": {
                            "name": "Signed out",
                            "gaia_id": "",
                            "user_name": ""
                        },
                        "Profile 3": {
                            "name": "Temporary",
                            "gaia_id": "gaia-three",
                            "user_name": "temp@example.com",
                            "is_ephemeral": true
                        },
                        "Profile 4": {
                            "name": "Personal",
                            "gaia_id": "gaia-four",
                            "user_name": "me@example.com"
                        },
                        "Profile 5": {
                            "name": "Managed",
                            "gaia_id": "gaia-five"
                        }
                    }
                }
            }"#,
        )
        .expect("valid Chrome Local State fixture");

        assert_eq!(profiles.len(), 4);
        assert_eq!(profiles[0].zz_profile, "chrome:Default");
        assert_eq!(profiles[0].menu_label(), "Work · dev@example.com");
        assert_eq!(profiles[1].zz_profile, "chrome:Profile 5");
        assert_eq!(profiles[1].menu_label(), "Managed");
        assert_eq!(profiles[2].zz_profile, "chrome:Profile 4");
        assert_eq!(profiles[2].menu_label(), "Personal · me@example.com");
        assert_eq!(profiles[3].zz_profile, "chrome:Profile 2");
        assert_eq!(profiles[3].menu_label(), "Signed out");
    }

    #[test]
    fn appends_cached_profiles_missing_from_chromes_order() {
        let profiles = parse_profiles(
            br#"{
                "profile": {
                    "profiles_order": ["Profile 2"],
                    "info_cache": {
                        "Profile 2": {"name": "Second", "user_name": "two@example.com"},
                        "Default": {"name": "First", "user_name": "one@example.com"}
                    }
                }
            }"#,
        )
        .expect("valid Chrome Local State fixture");

        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.zz_profile.as_str())
                .collect::<Vec<_>>(),
            ["chrome:Profile 2", "chrome:Default"]
        );
    }

    #[test]
    fn missing_cache_is_an_empty_discovery() {
        assert!(
            parse_profiles(br#"{"profile": {}}"#)
                .expect("valid empty fixture")
                .is_empty()
        );
    }

    #[test]
    fn sanitizes_control_characters_and_rejects_oversized_profile_keys() {
        let oversized_key = "x".repeat(80);
        let fixture = format!(
            r#"{{"profile":{{"info_cache":{{
                "Default":{{"name":"bad\nname","gaia_id":"id","user_name":"dev@example.com"}},
                "{oversized_key}":{{"name":"Work","gaia_id":"id","user_name":"dev@example.com"}}
            }}}}}}"#
        );
        let profiles = parse_profiles(fixture.as_bytes()).expect("valid bounded fixture");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].menu_label(), "dev@example.com");
    }

    #[test]
    fn rejects_storage_keys_that_could_escape_chromes_root() {
        for profile in [
            "chrome:../Default",
            "chrome:Profile/2",
            "chrome:Profile\\2",
            "chrome:.",
        ] {
            assert_eq!(chrome_storage_key(profile), None, "accepted {profile:?}");
        }
        assert_eq!(chrome_storage_key("chrome:Profile 2"), Some("Profile 2"));
    }

    #[test]
    fn cached_profile_labels_round_trip_without_chrome_account_ids() {
        let directory = tempdir().expect("temporary profile cache directory");
        let path = directory.path().join(PROFILE_CACHE_FILE_NAME);
        let profiles = vec![DetectedChromeProfile {
            zz_profile: "chrome:Default".to_owned(),
            display_name: "Work".to_owned(),
            email: Some("dev@example.com".to_owned()),
        }];

        save_cached_profiles_at(&path, &profiles).expect("save bounded profile cache");
        assert_eq!(
            load_cached_profiles_at(&path).expect("reload bounded profile cache"),
            profiles
        );
        let source = fs::read_to_string(path).expect("read serialized cache");
        assert!(!source.contains("gaia"));
    }

    #[test]
    fn cached_profile_labels_are_revalidated_and_deduplicated() {
        let profiles = sanitize_cached_profiles(vec![
            DetectedChromeProfile {
                zz_profile: "chrome:Default".to_owned(),
                display_name: " Work ".to_owned(),
                email: Some("dev@example.com".to_owned()),
            },
            DetectedChromeProfile {
                zz_profile: "chrome:Default".to_owned(),
                display_name: "Duplicate".to_owned(),
                email: None,
            },
            DetectedChromeProfile {
                zz_profile: "default".to_owned(),
                display_name: "Wrong namespace".to_owned(),
                email: None,
            },
            DetectedChromeProfile {
                zz_profile: "chrome:Profile 2".to_owned(),
                display_name: "Personal".to_owned(),
                email: Some("bad\nlabel".to_owned()),
            },
        ]);

        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].display_name, "Work");
        assert_eq!(profiles[0].email.as_deref(), Some("dev@example.com"));
        assert_eq!(profiles[1].display_name, "Personal");
        assert_eq!(profiles[1].email, None);
    }
}
