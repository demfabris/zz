//! Release check against GitHub and the in-app update offer.
//!
//! The check is one anonymous GET of the release list, ten seconds after
//! launch and daily after that. Applying an update reuses the shipped
//! installer: the offer opens a terminal window running whichever route
//! matches how this copy was installed, and the daemon keeps that pane alive
//! while the installer swaps the app underneath it.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

use gpui::{App, AsyncApp, Entity, Global, Window, prelude::*};
use semver::Version;
use serde::Deserialize;
use zz_protocol::CommandInvocation;
use zz_ui::{
    Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    notification::Notification,
};

use crate::{config, mux::client::MuxClient, user_data, window::toast};

const RELEASES_API: &str = "https://api.github.com/repos/demfabris/zz/releases?per_page=10";
const INSTALL_SCRIPT_URL: &str = "https://zzmux.sh/install.sh";
const INITIAL_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_hours(24);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const TOAST_KEY: &str = "update-available";
const DISMISSED_FILE_NAME: &str = "update-dismissed";
const LOG_TARGET: &str = "zz::update";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Channel {
    Stable,
    Beta,
}

impl Channel {
    fn of(version: &Version) -> Self {
        if version.pre.is_empty() {
            Self::Stable
        } else {
            Self::Beta
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Release {
    pub version: Version,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CheckState {
    Unchecked,
    Checking,
    UpToDate,
    Available(Release),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trigger {
    Scheduled,
    Manual,
}

pub(crate) struct UpdateState {
    mux: Entity<MuxClient>,
    current: Version,
    check: CheckState,
    dismissed: Option<Version>,
}

impl Global for UpdateState {}

/// What the Settings page shows.
pub(crate) struct Status {
    pub current: Version,
    pub channel: Channel,
    pub check: CheckState,
}

pub(crate) fn status(cx: &App) -> Option<Status> {
    let state = cx.try_global::<UpdateState>()?;
    Some(Status {
        current: state.current.clone(),
        channel: Channel::of(&state.current),
        check: state.check.clone(),
    })
}

/// Registers the update state and, when checks are enabled, starts the
/// daily check loop. Call once, as the workspace window opens.
pub(crate) fn start(mux: Entity<MuxClient>, cx: &mut App) {
    let current = match Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(version) => version,
        Err(error) => {
            log::warn!(target: LOG_TARGET, "update checks disabled: unparsable build version: {error}");
            return;
        }
    };
    cx.set_global(UpdateState {
        mux,
        current,
        check: CheckState::Unchecked,
        dismissed: read_dismissed(),
    });
    if !checks_enabled() {
        log::debug!(target: LOG_TARGET, "scheduled update checks are off for this build");
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(INITIAL_DELAY).await;
        loop {
            if cx.update(|cx| config::check_for_updates(cx)) {
                run_check(Trigger::Scheduled, cx).await;
            }
            cx.background_executor().timer(CHECK_INTERVAL).await;
        }
    })
    .detach();
}

/// A user-initiated check; reports the outcome as a toast either way.
pub(crate) fn check_now(cx: &mut App) {
    let checking = cx
        .try_global::<UpdateState>()
        .is_some_and(|state| state.check == CheckState::Checking);
    if checking {
        return;
    }
    cx.spawn(async move |cx| run_check(Trigger::Manual, cx).await)
        .detach();
}

/// Applies the offered release: a terminal window running the installer, or
/// the release page when nothing can be run in place.
pub(crate) fn install(window: &mut Window, cx: &mut App) {
    let Some(state) = cx.try_global::<UpdateState>() else {
        return;
    };
    let CheckState::Available(release) = &state.check else {
        return;
    };
    let release = release.clone();
    let channel = Channel::of(&state.current);
    let mux = state.mux.clone();
    let plan = install_plan(&release, channel, &Environment::detect());
    log::info!(target: LOG_TARGET, "applying update to {} via {plan:?}", release.version);
    match plan {
        InstallPlan::Terminal(command)
            if mux.read(cx).attached_session().is_some() && !config::quit_daemon_on_exit(cx) =>
        {
            let script = terminal_script(&command);
            mux.read(cx).execute(CommandInvocation::new(
                "new-window",
                ["-n", "update", "sh", "-c", script.as_str()],
            ));
        }
        InstallPlan::Terminal(_) => cx.open_url(&release.url),
        InstallPlan::Open(url) => cx.open_url(&url),
    }
    window.dismiss_notification(TOAST_KEY, cx);
}

fn checks_enabled() -> bool {
    match std::env::var("ZZ_UPDATE_CHECK") {
        Ok(value) => !matches!(value.as_str(), "" | "0" | "false" | "off"),
        Err(_) => !cfg!(debug_assertions),
    }
}

async fn run_check(trigger: Trigger, cx: &mut AsyncApp) {
    let Some(current) = cx.update(|cx| {
        cx.try_global::<UpdateState>()
            .map(|state| state.current.clone())
    }) else {
        return;
    };
    cx.update(|cx| set_check(CheckState::Checking, cx));
    let channel = Channel::of(&current);
    let result = cx
        .background_executor()
        .spawn(async move { fetch_latest(channel) })
        .await;
    cx.update(|cx| finish_check(result, trigger, cx));
}

fn set_check(check: CheckState, cx: &mut App) {
    if cx.has_global::<UpdateState>() {
        cx.global_mut::<UpdateState>().check = check;
    }
}

fn finish_check(result: Result<Option<Release>, String>, trigger: Trigger, cx: &mut App) {
    if !cx.has_global::<UpdateState>() {
        return;
    }
    let state = cx.global_mut::<UpdateState>();
    let current = state.current.clone();
    let (check, offer) = match result {
        Err(error) => (CheckState::Failed(error), None),
        Ok(None) => (
            CheckState::Failed("the release list carries no release".to_owned()),
            None,
        ),
        Ok(Some(release)) if release.version > current => {
            let offer =
                trigger == Trigger::Manual || state.dismissed.as_ref() != Some(&release.version);
            (
                CheckState::Available(release.clone()),
                offer.then_some(release),
            )
        }
        Ok(Some(_)) => (CheckState::UpToDate, None),
    };
    state.check = check.clone();

    match &check {
        CheckState::Failed(error) => {
            log::warn!(target: LOG_TARGET, "update check failed: {error}");
            if trigger == Trigger::Manual {
                toast::push(
                    Notification::error(format!("Could not check for updates: {error}")),
                    cx,
                );
            }
        }
        CheckState::UpToDate => {
            log::info!(target: LOG_TARGET, "zz {current} is the newest release");
            if trigger == Trigger::Manual {
                toast::push(
                    Notification::success(format!("zz {current} is the newest release")),
                    cx,
                );
            }
        }
        CheckState::Available(release) => {
            log::info!(target: LOG_TARGET, "zz {} is available (running {current})", release.version);
        }
        CheckState::Unchecked | CheckState::Checking => {}
    }
    if let Some(release) = offer {
        offer_update(&release, &current, cx);
    }
}

fn offer_update(release: &Release, current: &Version, cx: &mut App) {
    let version = release.version.clone();
    let url = release.url.clone();
    toast::push(
        Notification::new()
            .key(TOAST_KEY)
            .autohide(false)
            .title(format!("zz {version} is available"))
            .message(format!("You are running {current}."))
            .content(move |_, _, _| {
                let later_version = version.clone();
                let notes_url = url.clone();
                h_flex()
                    .gap_2()
                    .pt_1()
                    .child(
                        Button::new("update-install")
                            .primary()
                            .small()
                            .label("Update")
                            .on_click(|_, window, cx| install(window, cx)),
                    )
                    .child(
                        Button::new("update-notes")
                            .ghost()
                            .small()
                            .label("What's new")
                            .on_click(move |_, _, cx| cx.open_url(&notes_url)),
                    )
                    .child(
                        Button::new("update-later")
                            .ghost()
                            .small()
                            .label("Later")
                            .on_click(move |_, window, cx| {
                                dismiss(&later_version, cx);
                                window.dismiss_notification(TOAST_KEY, cx);
                            }),
                    )
                    .into_any_element()
            }),
        cx,
    );
}

fn dismiss(version: &Version, cx: &mut App) {
    if cx.has_global::<UpdateState>() {
        cx.global_mut::<UpdateState>().dismissed = Some(version.clone());
    }
    if let Err(error) = write_dismissed(version) {
        log::warn!(target: LOG_TARGET, "could not remember the dismissed release: {error}");
    }
}

fn dismissed_path() -> Option<PathBuf> {
    Some(
        user_data::platform_data_dir()?
            .join("zz")
            .join(DISMISSED_FILE_NAME),
    )
}

fn read_dismissed() -> Option<Version> {
    let text = fs::read_to_string(dismissed_path()?).ok()?;
    Version::parse(text.trim()).ok()
}

fn write_dismissed(version: &Version) -> io::Result<()> {
    let path = dismissed_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "could not resolve the current user's application-data directory",
        )
    })?;
    let parent = path
        .parent()
        .expect("the dismissed-release path has a parent");
    fs::create_dir_all(parent)?;
    user_data::restrict_directory_to_current_user(parent)?;
    fs::write(&path, format!("{version}\n"))?;
    user_data::restrict_to_current_user(&path)
}

fn fetch_latest(channel: Channel) -> Result<Option<Release>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into();
    let mut response = agent
        .get(RELEASES_API)
        .header("User-Agent", concat!("zz/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| error.to_string())?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    parse_releases(&body, channel)
}

#[derive(Deserialize)]
struct ReleaseEntry {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// The newest release the channel accepts: stable ignores anything marked
/// prerelease or tagged with a prerelease suffix; beta takes everything.
fn parse_releases(json: &str, channel: Channel) -> Result<Option<Release>, String> {
    let entries: Vec<ReleaseEntry> =
        serde_json::from_str(json).map_err(|error| format!("unexpected release list: {error}"))?;
    Ok(entries
        .into_iter()
        .filter(|entry| !entry.draft)
        .filter_map(|entry| {
            let version = Version::parse(entry.tag_name.strip_prefix('v')?).ok()?;
            let stable = !entry.prerelease && version.pre.is_empty();
            (channel == Channel::Beta || stable).then_some(Release {
                version,
                url: entry.html_url,
            })
        })
        .max_by(|a, b| a.version.cmp(&b.version)))
}

/// Facts about this copy of zz that pick the install route.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Environment {
    os: &'static str,
    executable: Option<PathBuf>,
    /// The Homebrew prefix whose Caskroom holds a zz cask, and that cask's token.
    homebrew: Option<(PathBuf, &'static str)>,
    pacman_owned: bool,
    appimage: bool,
    aur_helper: Option<&'static str>,
}

impl Environment {
    fn detect() -> Self {
        let executable = std::env::current_exe().ok();
        Self {
            os: std::env::consts::OS,
            homebrew: ["/opt/homebrew", "/usr/local"]
                .into_iter()
                .map(PathBuf::from)
                .find_map(|prefix| {
                    ["zz", "zz@beta"]
                        .into_iter()
                        .find(|cask| prefix.join("Caskroom").join(cask).is_dir())
                        .map(|cask| (prefix, cask))
                }),
            pacman_owned: fs::read_dir("/var/lib/pacman/local").is_ok_and(|entries| {
                entries.flatten().any(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.starts_with("zz-bin-") || name.starts_with("zz-beta-bin-")
                })
            }),
            appimage: std::env::var_os("APPIMAGE").is_some(),
            aur_helper: ["paru", "yay"].into_iter().find(|helper| on_path(helper)),
            executable,
        }
    }
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InstallPlan {
    /// A shell command to run in a terminal window.
    Terminal(String),
    /// A URL to open when nothing can be run in place.
    Open(String),
}

fn install_plan(release: &Release, channel: Channel, environment: &Environment) -> InstallPlan {
    match environment.os {
        "macos" => match &environment.homebrew {
            Some((prefix, cask)) => InstallPlan::Terminal(format!(
                "{} upgrade --cask {cask}",
                prefix.join("bin").join("brew").display()
            )),
            None => InstallPlan::Terminal(install_script_command(channel, None)),
        },
        "linux" if environment.appimage => InstallPlan::Open(release.url.clone()),
        "linux" if environment.pacman_owned => match environment.aur_helper {
            Some(helper) => InstallPlan::Terminal(format!(
                "{helper} -S {}",
                match channel {
                    Channel::Stable => "zz-bin",
                    Channel::Beta => "zz-beta-bin",
                }
            )),
            None => InstallPlan::Open(release.url.clone()),
        },
        "linux" => InstallPlan::Terminal(install_script_command(
            channel,
            environment.executable.as_deref().and_then(tarball_prefix),
        )),
        "windows" => InstallPlan::Open(format!(
            "https://github.com/demfabris/zz/releases/download/v{version}/zz-{version}-windows-x64-setup.exe",
            version = release.version
        )),
        _ => InstallPlan::Open(release.url.clone()),
    }
}

/// The unpack prefix of a tarball install (`<prefix>/lib/zz/zz`); `None` for
/// the apt-managed `/usr` tree, which the installer upgrades through apt.
fn tarball_prefix(executable: &Path) -> Option<&Path> {
    let prefix = executable.parent()?.parent()?.parent()?;
    (executable.ends_with("lib/zz/zz") && prefix != Path::new("/usr")).then_some(prefix)
}

fn install_script_command(channel: Channel, prefix: Option<&Path>) -> String {
    let mut arguments = Vec::new();
    if channel == Channel::Beta {
        arguments.push("--beta".to_owned());
    }
    if let Some(prefix) = prefix {
        arguments.push(format!("--prefix '{}'", prefix.display()));
    }
    if arguments.is_empty() {
        format!("curl -fsSL {INSTALL_SCRIPT_URL} | sh")
    } else {
        format!(
            "curl -fsSL {INSTALL_SCRIPT_URL} | sh -s -- {}",
            arguments.join(" ")
        )
    }
}

/// Wraps the install command so the window outlives it: the log stays
/// readable, and a failure says so instead of closing the pane.
fn terminal_script(command: &str) -> String {
    let quoted = command.replace('\'', "'\\''");
    format!(
        "printf '%s\\n' '$ {quoted}'\n\
         {command}\n\
         status=$?\n\
         if [ \"$status\" -ne 0 ]; then printf '\\nzz update failed (exit %s).\\n' \"$status\"; fi\n\
         printf '\\nPress Enter to close this window.\\n'\n\
         read -r _\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).expect("valid version")
    }

    fn release(tag: &str, prerelease: bool, draft: bool) -> String {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/demfabris/zz/releases/tag/{tag}","draft":{draft},"prerelease":{prerelease}}}"#
        )
    }

    fn releases(entries: &[String]) -> String {
        format!("[{}]", entries.join(","))
    }

    #[test]
    fn channel_follows_the_running_build() {
        assert_eq!(Channel::of(&version("0.3.1")), Channel::Stable);
        assert_eq!(Channel::of(&version("0.3.2-beta.1")), Channel::Beta);
    }

    #[test]
    fn stable_channel_skips_prereleases_and_drafts() {
        let json = releases(&[
            release("v0.4.0", false, true),
            release("v0.3.2-beta.1", true, false),
            release("v0.3.1", false, false),
            release("v0.3.0", false, false),
        ]);
        let newest = parse_releases(&json, Channel::Stable)
            .expect("parses")
            .expect("finds a release");
        assert_eq!(newest.version, version("0.3.1"));
        assert_eq!(
            newest.url,
            "https://github.com/demfabris/zz/releases/tag/v0.3.1"
        );
    }

    #[test]
    fn stable_channel_distrusts_an_unflagged_prerelease_tag() {
        let json = releases(&[
            release("v0.3.2-beta.1", false, false),
            release("v0.3.1", false, false),
        ]);
        let newest = parse_releases(&json, Channel::Stable).unwrap().unwrap();
        assert_eq!(newest.version, version("0.3.1"));
    }

    #[test]
    fn beta_channel_takes_the_newest_of_everything() {
        let json = releases(&[
            release("v0.3.1", false, false),
            release("v0.3.2-beta.1", true, false),
            release("v0.3.2-beta.2", true, false),
        ]);
        let newest = parse_releases(&json, Channel::Beta).unwrap().unwrap();
        assert_eq!(newest.version, version("0.3.2-beta.2"));

        let promoted = releases(&[
            release("v0.3.2", false, false),
            release("v0.3.2-beta.2", true, false),
        ]);
        let newest = parse_releases(&promoted, Channel::Beta).unwrap().unwrap();
        assert_eq!(newest.version, version("0.3.2"));
    }

    #[test]
    fn foreign_tags_and_bad_json_are_handled() {
        let json = releases(&[
            release("nightly", false, false),
            release("v0.3.1", false, false),
        ]);
        let newest = parse_releases(&json, Channel::Stable).unwrap().unwrap();
        assert_eq!(newest.version, version("0.3.1"));
        assert_eq!(parse_releases("[]", Channel::Stable), Ok(None));
        assert!(parse_releases(r#"{"message":"rate limited"}"#, Channel::Stable).is_err());
    }

    fn offered() -> Release {
        Release {
            version: version("0.3.2"),
            url: "https://github.com/demfabris/zz/releases/tag/v0.3.2".to_owned(),
        }
    }

    #[test]
    fn macos_uses_the_installer_unless_homebrew_owns_the_app() {
        let plain = Environment {
            os: "macos",
            ..Environment::default()
        };
        assert_eq!(
            install_plan(&offered(), Channel::Stable, &plain),
            InstallPlan::Terminal("curl -fsSL https://zzmux.sh/install.sh | sh".to_owned())
        );
        assert_eq!(
            install_plan(&offered(), Channel::Beta, &plain),
            InstallPlan::Terminal(
                "curl -fsSL https://zzmux.sh/install.sh | sh -s -- --beta".to_owned()
            )
        );
        let brewed = Environment {
            os: "macos",
            homebrew: Some((PathBuf::from("/opt/homebrew"), "zz@beta")),
            ..Environment::default()
        };
        assert_eq!(
            install_plan(&offered(), Channel::Beta, &brewed),
            InstallPlan::Terminal("/opt/homebrew/bin/brew upgrade --cask zz@beta".to_owned())
        );
    }

    #[test]
    fn linux_routes_by_package_owner() {
        let deb = Environment {
            os: "linux",
            executable: Some(PathBuf::from("/usr/lib/zz/zz")),
            ..Environment::default()
        };
        assert_eq!(
            install_plan(&offered(), Channel::Stable, &deb),
            InstallPlan::Terminal("curl -fsSL https://zzmux.sh/install.sh | sh".to_owned())
        );
        let tarball = Environment {
            os: "linux",
            executable: Some(PathBuf::from("/home/me/.local/lib/zz/zz")),
            ..Environment::default()
        };
        assert_eq!(
            install_plan(&offered(), Channel::Beta, &tarball),
            InstallPlan::Terminal(
                "curl -fsSL https://zzmux.sh/install.sh | sh -s -- --beta --prefix '/home/me/.local'"
                    .to_owned()
            )
        );
        let aur = Environment {
            os: "linux",
            executable: Some(PathBuf::from("/usr/lib/zz/zz")),
            pacman_owned: true,
            aur_helper: Some("paru"),
            ..Environment::default()
        };
        assert_eq!(
            install_plan(&offered(), Channel::Stable, &aur),
            InstallPlan::Terminal("paru -S zz-bin".to_owned())
        );
        let aur_without_helper = Environment {
            aur_helper: None,
            ..aur
        };
        assert_eq!(
            install_plan(&offered(), Channel::Stable, &aur_without_helper),
            InstallPlan::Open(offered().url)
        );
        let appimage = Environment {
            os: "linux",
            appimage: true,
            ..Environment::default()
        };
        assert_eq!(
            install_plan(&offered(), Channel::Stable, &appimage),
            InstallPlan::Open(offered().url)
        );
    }

    #[test]
    fn windows_opens_the_installer_download() {
        let windows = Environment {
            os: "windows",
            ..Environment::default()
        };
        assert_eq!(
            install_plan(&offered(), Channel::Stable, &windows),
            InstallPlan::Open(
                "https://github.com/demfabris/zz/releases/download/v0.3.2/zz-0.3.2-windows-x64-setup.exe"
                    .to_owned()
            )
        );
    }

    #[test]
    fn terminal_script_keeps_the_window_open_and_reports_failure() {
        let script = terminal_script("false");
        assert!(script.contains("printf '%s\\n' '$ false'\nfalse\nstatus=$?\n"));
        assert!(script.contains("zz update failed (exit %s)"));
        assert!(script.ends_with("read -r _\n"));
    }
}
