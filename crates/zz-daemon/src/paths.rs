//! Discovery of the zz-owned mux configuration file (`zz/mux.conf`).

use std::{
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use crate::fleet_hosts::atomic_write;

pub const MAX_TMUX_IMPORT_BYTES: usize = 1024 * 1024;

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

#[must_use]
fn tmux_config_candidates_for(home: Option<&Path>, xdg_config_home: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(3);
    if let Some(home) = home {
        candidates.push(home.join(".tmux.conf"));
    }
    if let Some(xdg) = xdg_config_home {
        candidates.push(xdg.join("tmux/tmux.conf"));
    }
    if let Some(home) = home {
        let fallback = home.join(".config/tmux/tmux.conf");
        if !candidates.contains(&fallback) {
            candidates.push(fallback);
        }
    }
    candidates
}

#[must_use]
pub fn discover_tmux_config() -> Option<PathBuf> {
    let home = nonempty_env("HOME");
    let xdg_config_home = nonempty_env("XDG_CONFIG_HOME");
    tmux_config_candidates_for(home.as_deref(), xdg_config_home.as_deref())
        .into_iter()
        .find(|candidate| candidate.is_file())
}

pub fn copy_tmux_config_into(donor: &Path, target: &Path) -> io::Result<u64> {
    let file = std::fs::File::open(donor)?;
    let byte_limit = u64::try_from(MAX_TMUX_IMPORT_BYTES).unwrap_or(u64::MAX - 1);
    let mut contents = Vec::new();
    file.take(byte_limit + 1).read_to_end(&mut contents)?;
    if contents.len() > MAX_TMUX_IMPORT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("tmux configuration exceeds the {MAX_TMUX_IMPORT_BYTES}-byte import limit"),
        ));
    }
    atomic_write(target, &contents)?;
    Ok(contents.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn absolute_test_root(name: &str) -> PathBuf {
        std::env::current_dir()
            .expect("current directory")
            .join("target")
            .join("mux-config-path-tests")
            .join(name)
    }

    fn expected_mux_config(base: &Path) -> PathBuf {
        base.join(MUX_CONFIG_DIRECTORY_NAME)
            .join(MUX_CONFIG_FILE_NAME)
    }

    #[test]
    fn unix_candidates_prefer_xdg_and_dedupe() {
        let home = absolute_test_root("unix-home");
        let xdg = home.join(".config");
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
            vec![expected_mux_config(&xdg)],
            "XDG and ~/.config resolve to the same path and must dedupe",
        );
    }

    #[test]
    fn macos_candidates_keep_xdg_home_ahead_of_application_support() {
        let home = absolute_test_root("macos-home");
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
                expected_mux_config(&home.join(".config")),
                expected_mux_config(&home.join("Library").join("Application Support")),
            ],
        );
    }

    #[test]
    fn windows_candidates_follow_appdata_precedence() {
        let root = absolute_test_root("windows");
        let appdata = root.join("AppData").join("Roaming");
        let local = root.join("AppData").join("Local");
        let profile = root.join("Users").join("u");
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
                expected_mux_config(&appdata),
                expected_mux_config(&local),
                expected_mux_config(&profile.join(".config")),
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

    #[test]
    fn tmux_candidates_prefer_home_dotfile_then_xdg() {
        let home = Path::new("/home/u");
        let xdg = Path::new("/home/u/xdg");
        assert_eq!(
            tmux_config_candidates_for(Some(home), Some(xdg)),
            vec![
                PathBuf::from("/home/u/.tmux.conf"),
                PathBuf::from("/home/u/xdg/tmux/tmux.conf"),
                PathBuf::from("/home/u/.config/tmux/tmux.conf"),
            ],
        );
    }

    #[test]
    fn tmux_candidates_dedupe_xdg_matching_home_config() {
        let home = Path::new("/home/u");
        let xdg = Path::new("/home/u/.config");
        assert_eq!(
            tmux_config_candidates_for(Some(home), Some(xdg)),
            vec![
                PathBuf::from("/home/u/.tmux.conf"),
                PathBuf::from("/home/u/.config/tmux/tmux.conf"),
            ],
        );
    }

    #[test]
    fn tmux_copy_is_verbatim_and_overwrites_on_reimport() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let donor = directory.path().join(".tmux.conf");
        let target = directory.path().join("zz/mux.conf");
        let first = b"set -g prefix C-a\nbind h select-pane -L\n";
        std::fs::write(&donor, first).expect("write donor");

        let bytes = copy_tmux_config_into(&donor, &target).expect("copy donor");
        assert_eq!(bytes, first.len() as u64);
        assert_eq!(std::fs::read(&target).expect("read copy"), first);

        let second = b"set -g prefix C-b\n";
        std::fs::write(&donor, second).expect("update donor");
        copy_tmux_config_into(&donor, &target).expect("overwrite copy");
        assert_eq!(
            std::fs::read(&target).expect("read overwritten copy"),
            second
        );
    }

    #[test]
    fn oversized_tmux_donor_is_rejected() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let big = directory.path().join("big.conf");
        std::fs::write(&big, vec![b'#'; MAX_TMUX_IMPORT_BYTES + 1]).expect("write big donor");
        let error = copy_tmux_config_into(&big, &directory.path().join("zz/mux.conf"))
            .expect_err("donor past the bound must fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }
}
