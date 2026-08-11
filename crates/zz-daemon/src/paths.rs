//! Discovery of the zz-owned mux configuration file (`zz/mux.conf`).

use std::{
    fs,
    path::{Path, PathBuf},
};

const MUX_CONFIG_DIRECTORY_NAME: &str = "zz";
const MUX_CONFIG_FILE_NAME: &str = "mux.conf";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MuxConfigPlatform {
    #[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
    Unix,
    #[cfg_attr(not(test), allow(dead_code))]
    Macos,
    #[cfg_attr(not(test), allow(dead_code))]
    Windows,
}

fn current_platform() -> MuxConfigPlatform {
    if cfg!(target_os = "macos") {
        MuxConfigPlatform::Macos
    } else if cfg!(target_os = "windows") {
        MuxConfigPlatform::Windows
    } else {
        MuxConfigPlatform::Unix
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct MuxConfigEnvironment<'a> {
    xdg_config_home: Option<&'a Path>,
    home: Option<&'a Path>,
    appdata: Option<&'a Path>,
    local_appdata: Option<&'a Path>,
    user_profile: Option<&'a Path>,
}

fn environment_paths() -> [Option<PathBuf>; 5] {
    [
        nonempty_env("XDG_CONFIG_HOME"),
        nonempty_env("HOME"),
        nonempty_env("APPDATA"),
        nonempty_env("LOCALAPPDATA"),
        nonempty_env("USERPROFILE"),
    ]
}

fn nonempty_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(windows))]
pub(crate) fn home_directory() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

// Windows has no `HOME`: `USERPROFILE` is the real home, `HOME` only a shell fallback.
#[cfg(windows)]
pub(crate) fn home_directory() -> Option<PathBuf> {
    nonempty_env("USERPROFILE").or_else(|| nonempty_env("HOME"))
}

/// Ordered candidate locations for `zz/mux.conf`, matching the client's `zz/config` order.
#[must_use]
pub fn mux_config_candidates() -> Vec<PathBuf> {
    let [xdg_config_home, home, appdata, local_appdata, user_profile] = environment_paths();
    mux_config_candidates_for(
        current_platform(),
        MuxConfigEnvironment {
            xdg_config_home: xdg_config_home.as_deref(),
            home: home.as_deref(),
            appdata: appdata.as_deref(),
            local_appdata: local_appdata.as_deref(),
            user_profile: user_profile.as_deref(),
        },
    )
}

fn mux_config_candidates_for(
    platform: MuxConfigPlatform,
    environment: MuxConfigEnvironment<'_>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_candidate(&mut candidates, environment.xdg_config_home);
    match platform {
        MuxConfigPlatform::Unix => {
            push_home_candidate(&mut candidates, environment.home);
        }
        MuxConfigPlatform::Macos => {
            push_home_candidate(&mut candidates, environment.home);
            if let Some(home) = environment.home {
                push_candidate_path(&mut candidates, &home.join("Library/Application Support"));
            }
        }
        MuxConfigPlatform::Windows => {
            push_candidate(&mut candidates, environment.appdata);
            push_candidate(&mut candidates, environment.local_appdata);
            push_home_candidate(&mut candidates, environment.user_profile);
            push_home_candidate(&mut candidates, environment.home);
        }
    }
    candidates
}

fn push_home_candidate(candidates: &mut Vec<PathBuf>, home: Option<&Path>) {
    if let Some(home) = home {
        push_candidate_path(candidates, &home.join(".config"));
    }
}

fn push_candidate(candidates: &mut Vec<PathBuf>, base: Option<&Path>) {
    if let Some(base) = base {
        push_candidate_path(candidates, base);
    }
}

fn push_candidate_path(candidates: &mut Vec<PathBuf>, base: &Path) {
    if !base.is_absolute() {
        return;
    }
    let candidate = base
        .join(MUX_CONFIG_DIRECTORY_NAME)
        .join(MUX_CONFIG_FILE_NAME);
    if !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

/// First existing `zz/mux.conf` candidate, sourced at startup and on `reload-config`.
#[must_use]
pub fn default_mux_config() -> Option<PathBuf> {
    mux_config_candidates()
        .into_iter()
        .find(|candidate| candidate.is_file())
}

/// Where a config import lands: the first existing candidate, else the first constructible one.
#[must_use]
pub fn mux_config_write_path() -> Option<PathBuf> {
    let candidates = mux_config_candidates();
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .or_else(|| candidates.into_iter().next())
}

pub(crate) fn is_default_mux_config(path: &Path) -> bool {
    let canonical = fs::canonicalize(path);
    mux_config_candidates().iter().any(|candidate| {
        match (&canonical, fs::canonicalize(candidate)) {
            (Ok(path), Ok(candidate)) => *path == candidate,
            _ => path == candidate,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_candidates_prefer_xdg_and_dedupe() {
        let xdg = PathBuf::from("/home/u/.config");
        let home = PathBuf::from("/home/u");
        let candidates = mux_config_candidates_for(
            MuxConfigPlatform::Unix,
            MuxConfigEnvironment {
                xdg_config_home: Some(&xdg),
                home: Some(&home),
                ..MuxConfigEnvironment::default()
            },
        );
        assert_eq!(
            candidates,
            vec![PathBuf::from("/home/u/.config/zz/mux.conf")],
            "XDG and ~/.config resolve to the same path and must dedupe",
        );
    }

    #[test]
    fn macos_candidates_keep_xdg_home_ahead_of_application_support() {
        let home = PathBuf::from("/Users/u");
        let candidates = mux_config_candidates_for(
            MuxConfigPlatform::Macos,
            MuxConfigEnvironment {
                home: Some(&home),
                ..MuxConfigEnvironment::default()
            },
        );
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/Users/u/.config/zz/mux.conf"),
                PathBuf::from("/Users/u/Library/Application Support/zz/mux.conf"),
            ],
        );
    }

    #[test]
    fn windows_candidates_follow_appdata_precedence() {
        let appdata = PathBuf::from("/win/AppData/Roaming");
        let local = PathBuf::from("/win/AppData/Local");
        let profile = PathBuf::from("/win/Users/u");
        let candidates = mux_config_candidates_for(
            MuxConfigPlatform::Windows,
            MuxConfigEnvironment {
                appdata: Some(&appdata),
                local_appdata: Some(&local),
                user_profile: Some(&profile),
                ..MuxConfigEnvironment::default()
            },
        );
        assert_eq!(
            candidates,
            vec![
                PathBuf::from("/win/AppData/Roaming/zz/mux.conf"),
                PathBuf::from("/win/AppData/Local/zz/mux.conf"),
                PathBuf::from("/win/Users/u/.config/zz/mux.conf"),
            ],
        );
    }

    #[test]
    fn relative_bases_are_rejected() {
        let relative = PathBuf::from("relative/.config");
        let candidates = mux_config_candidates_for(
            MuxConfigPlatform::Unix,
            MuxConfigEnvironment {
                xdg_config_home: Some(&relative),
                ..MuxConfigEnvironment::default()
            },
        );
        assert!(candidates.is_empty());
    }
}
