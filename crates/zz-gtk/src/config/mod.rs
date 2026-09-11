//! `zz/config`, the file this client shares with the zz app.
//!
//! The single apply path is the poll. Nothing here mutates live state on a
//! GUI edit: a settings row writes the file and the 500 ms poller reads it back
//! and republishes, so a hand edit in an editor and a click in the window are
//! literally the same code path — which is the property the desktop has and the
//! reason its settings surface never drifts from the file.
//!
//! [`file`] is lifted from the desktop; [`schema`] is the key table.

pub mod file;
pub mod import;
pub mod schema;

use std::{
    cell::RefCell,
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use zz_client::{
    CHROME_TABLES, ChromeAction, ChromeKey, ChromeKeymap, ChromeProfile, StatusBarAlignment,
    StatusBarClock, StatusBarSettings,
};
use zz_daemon::{HostEntry, RejectedHost, apply_fleet_host_entry, validate_fleet_host};
use zz_protocol::{ConfigOverrideEntry, MuxOptionKey};
use zz_terminal::AppearanceConfigKey;

pub use file::{MAX_CONFIG_BYTES, POLL_INTERVAL};
pub use schema::{Kind, Owner, Page, Setting, Support};

#[derive(Clone, Debug)]
pub struct ClientSettings {
    pub ui_font_family: Option<String>,
    pub animations: bool,
    pub browser_search: zz_browser::SearchProvider,
    pub browser_egress: bool,
    pub status: StatusBarSettings,
    pub agent_directory: Option<PathBuf>,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            ui_font_family: None,
            animations: true,
            browser_search: zz_browser::SearchProvider::default(),
            browser_egress: true,
            status: StatusBarSettings {
                show_update: false,
                ..StatusBarSettings::default()
            },
            agent_directory: None,
        }
    }
}

thread_local! {
    static CURRENT: RefCell<ClientSettings> = RefCell::new(ClientSettings::default());
}

pub fn publish(state: &State) {
    CURRENT.with_borrow_mut(|current| *current = state.client_settings());
}

pub fn current() -> ClientSettings {
    CURRENT.with_borrow(Clone::clone)
}

/// Where an effective value came from. The two client-local variants are the
/// desktop's `ConfigProvenance`; the rest are the daemon's own answer for the
/// keys it owns, reported rather than guessed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Provenance {
    #[default]
    Default,
    Override,
    ThemeFile,
    Ghostty,
    TmuxConfig,
    RuntimeCommand,
}

impl Provenance {
    pub const fn badge(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Override => "Overridden",
            Self::ThemeFile => "From a theme file",
            Self::Ghostty => "From Ghostty",
            Self::TmuxConfig => "From mux.conf",
            Self::RuntimeCommand => "Set at runtime",
        }
    }
}

/// One parse of the file. Values are kept as the raw trimmed text the file
/// carried: client keys are interpreted by the widget that renders them, and
/// daemon keys are never interpreted at all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub path: Option<PathBuf>,
    values: BTreeMap<String, String>,
    daemon_entries: Vec<ConfigOverrideEntry>,
    chrome_overrides: Vec<ChromeOverride>,
    chrome_errors: Vec<(usize, String)>,
    hosts: Vec<HostEntry>,
    rejected_hosts: Vec<RejectedHost>,
    malformed_lines: Vec<usize>,
}

impl State {
    /// The raw text the file assigns to `key`, last occurrence winning.
    #[must_use]
    pub fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    #[must_use]
    pub fn is_overridden(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// The ordered raw vector for `SetConfigOverrides`: every appearance and
    /// mux entry in file order, duplicates kept, values unparsed. Order and
    /// duplicates are load-bearing — the daemon applies last-writer per key and
    /// cumulative keys like `palette` need every occurrence.
    #[must_use]
    pub fn daemon_entries(&self) -> &[ConfigOverrideEntry] {
        &self.daemon_entries
    }

    /// Every `host-<name>` line that names a reachable-looking daemon, in file
    /// order, resolved by the daemon's own validator so this client and the
    /// desktop agree byte for byte on what a fleet is.
    #[must_use]
    pub fn fleet_hosts(&self) -> &[HostEntry] {
        &self.hosts
    }

    #[must_use]
    pub fn rejected_hosts(&self) -> &[RejectedHost] {
        &self.rejected_hosts
    }

    #[must_use]
    pub fn malformed_lines(&self) -> &[usize] {
        &self.malformed_lines
    }

    #[must_use]
    pub fn chrome_keymap(&self) -> ChromeKeymap {
        let mut chrome = ChromeKeymap::for_profile(ChromeProfile::DESKTOP);
        for entry in &self.chrome_overrides {
            match entry {
                ChromeOverride::Bind { table, key, action } => {
                    chrome
                        .bind(table, key, action)
                        .expect("validated chrome action");
                }
                ChromeOverride::Unbind { table, key } => {
                    chrome.unbind(table, key);
                }
            }
        }
        chrome
    }

    #[must_use]
    pub fn boolean(&self, key: &str, default: bool) -> bool {
        self.value(key).and_then(parse_boolean).unwrap_or(default)
    }

    pub fn client_settings(&self) -> ClientSettings {
        let defaults = StatusBarSettings::default();
        ClientSettings {
            ui_font_family: self.value("ui-font-family").and_then(parse_config_string),
            animations: self.boolean("animations", true),
            browser_search: self
                .value("browser-search-provider")
                .and_then(zz_browser::SearchProvider::parse)
                .unwrap_or_default(),
            browser_egress: self.boolean("browser-egress", true),
            agent_directory: self
                .value("agent-working-directory")
                .and_then(parse_config_string)
                .map(PathBuf::from)
                .filter(|path| path.is_absolute()),
            status: StatusBarSettings {
                show_session: self.boolean("status-show-session", defaults.show_session),
                badges: self.boolean("status-badges", defaults.badges),
                show_agents: self.boolean("status-agents", defaults.show_agents),
                show_host: self.boolean("status-host", defaults.show_host),
                show_update: false,
                alignment: match self.value("status-align") {
                    Some("center") => StatusBarAlignment::Center,
                    _ => StatusBarAlignment::Left,
                },
                clock: match self.value("status-clock") {
                    Some("12-hour") => StatusBarClock::TwelveHour,
                    Some("time-date") => StatusBarClock::TimeAndDate,
                    Some("off") => StatusBarClock::Off,
                    _ => StatusBarClock::TwentyFourHour,
                },
            },
        }
    }

    #[must_use]
    pub fn number(&self, key: &str, default: f32) -> f32 {
        self.value(key)
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(default)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ChromeOverride {
    Bind {
        table: &'static str,
        key: String,
        action: String,
    },
    Unbind {
        table: &'static str,
        key: String,
    },
}

/// Parse one config source. Deliberately forgiving: this client models a
/// subset of the keys the desktop does, so an unrecognized key is another
/// surface's business, not an error. Only a line that cannot be a `key = value`
/// at all is reported.
#[must_use]
pub fn parse(source: &str) -> State {
    let mut state = State::default();
    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            state.malformed_lines.push(index + 1);
            continue;
        };
        let key = key.trim();
        let value = file::value_without_comment(value).trim();
        let chrome = match key {
            "chrome-keybind" => Some(parse_chrome_bind(value)),
            "chrome-unbind" => Some(parse_chrome_unbind(value)),
            _ => None,
        };
        if let Some(chrome) = chrome {
            match chrome {
                Ok(chrome) => state.chrome_overrides.push(chrome),
                Err(error) => state.chrome_errors.push((index + 1, error)),
            }
        }
        if is_daemon_owned(key) {
            state
                .daemon_entries
                .push((key.to_owned(), value.to_owned()));
        }
        if let Some(name) = key.strip_prefix("host-") {
            apply_fleet_host_entry(
                &mut state.hosts,
                &mut state.rejected_hosts,
                key,
                name,
                value,
            );
        }
        if let Some(setting) = schema::SETTINGS
            .iter()
            .find(|setting| setting.key == key && setting.owner == Owner::Client)
        {
            let normalized = normalize_client_value(setting, value);
            if let Some(value) = normalized {
                state.values.insert(key.to_owned(), value);
            } else {
                log::warn!("Ignoring invalid {key} on config line {}", index + 1);
            }
        } else {
            state.values.insert(key.to_owned(), value.to_owned());
        }
    }
    state
}

fn normalize_client_value(setting: &Setting, value: &str) -> Option<String> {
    match setting.kind {
        Kind::Toggle { .. } => parse_boolean(value).map(|value| schema::boolean(value).to_owned()),
        Kind::Choice { options, .. } => options
            .iter()
            .any(|option| option.value == value)
            .then(|| value.to_owned()),
        Kind::Text { .. } if setting.key == "ui-font-family" => parse_config_string(value)
            .filter(|family| !family.trim().is_empty() && !family.chars().any(char::is_control))
            .map(|_| value.to_owned()),
        Kind::Text { .. } if setting.key == "agent-working-directory" => parse_config_string(value)
            .filter(|path| {
                Path::new(path).is_absolute() && path.len() <= zz_protocol::MAX_GUI_TEXT_BYTES
            })
            .map(|_| value.to_owned()),
        _ => Some(value.to_owned()),
    }
}

fn parse_boolean(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn parse_config_string(value: &str) -> Option<String> {
    let value = value.trim();
    let first = value.chars().next()?;
    if !matches!(first, '\'' | '"') {
        return Some(value.to_owned());
    }
    if value.len() < 2 || !value.ends_with(first) {
        return None;
    }
    let mut result = String::new();
    let mut escaped = false;
    for character in value[1..value.len() - 1].chars() {
        if escaped {
            result.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            result.push(character);
        }
    }
    (!escaped && !result.is_empty()).then_some(result)
}

fn parse_chrome_bind(value: &str) -> Result<ChromeOverride, String> {
    let (target, action) = value
        .rsplit_once('=')
        .ok_or_else(|| "expected `<table>:<key>=<action>`".to_owned())?;
    let (table, key) = parse_chrome_target(target)?;
    let action = action.trim();
    if ChromeAction::from_name(action).is_none() {
        return Err(format!("unknown chrome action `{action}`"));
    }
    Ok(ChromeOverride::Bind {
        table,
        key,
        action: action.to_owned(),
    })
}

fn parse_chrome_unbind(value: &str) -> Result<ChromeOverride, String> {
    let (table, key) = parse_chrome_target(value)?;
    Ok(ChromeOverride::Unbind { table, key })
}

fn parse_chrome_target(target: &str) -> Result<(&'static str, String), String> {
    let (table, key) = target
        .split_once(':')
        .ok_or_else(|| "expected `<table>:<key>`".to_owned())?;
    let table = table.trim();
    let table = CHROME_TABLES
        .into_iter()
        .find(|known| *known == table)
        .ok_or_else(|| {
            format!(
                "unknown chrome table `{table}`; expected one of {}",
                CHROME_TABLES.join(", "),
            )
        })?;
    let key = key.trim();
    let chord =
        ChromeKey::parse(key).ok_or_else(|| format!("`{key}` is not a chord zz can bind"))?;
    Ok((table, chord.to_string()))
}

/// The daemon's key set, taken from the daemon's own enums rather than a list
/// restated here — `partition_config_overrides` accepts exactly these and warns
/// about anything else.
#[must_use]
pub fn is_daemon_owned(key: &str) -> bool {
    AppearanceConfigKey::from_config_key(key).is_some()
        || MuxOptionKey::from_config_key(key).is_some()
}

/// The file, plus everything needed to notice it changed.
pub struct Store {
    candidates: Vec<PathBuf>,
    stamp: file::Stamp,
    state: State,
}

impl Default for Store {
    fn default() -> Self {
        Self::load()
    }
}

impl Store {
    #[must_use]
    pub fn load() -> Self {
        Self::for_candidates(file::candidates())
    }

    #[must_use]
    pub fn for_candidates(candidates: Vec<PathBuf>) -> Self {
        let stamp = file::Stamp::detect(&candidates);
        let state = read_state(&stamp);
        Self {
            candidates,
            stamp,
            state,
        }
    }

    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }

    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.state.path.as_deref()
    }

    /// Re-stamp and, when the file moved, grew, or was touched, re-read it.
    /// True when the state was replaced. Two `stat` calls when nothing changed,
    /// which is why this can sit on the main loop instead of a worker thread.
    pub fn poll(&mut self) -> bool {
        let next = file::Stamp::detect(&self.candidates);
        if next == self.stamp {
            return false;
        }
        self.stamp = next;
        self.state = read_state(&self.stamp);
        true
    }

    /// Forget the current stamp so the next [`Self::poll`] re-reads whatever is
    /// on disk. A write this client just made may land inside the same
    /// filesystem timestamp tick at the same length, and the poll must not be
    /// able to miss it.
    pub fn invalidate(&mut self) {
        self.stamp = file::Stamp::default();
    }
}

fn read_state(stamp: &file::Stamp) -> State {
    let Some(path) = stamp.path.as_deref() else {
        return State::default();
    };
    let mut state = match file::read_source(path) {
        Ok(source) => parse(&source),
        Err(error) => {
            log::warn!(
                target: "zz_gtk::config",
                "could not read {}: {error}; using built-in defaults",
                path.display(),
            );
            State::default()
        }
    };
    for line in state.malformed_lines() {
        log::warn!(
            target: "zz_gtk::config",
            "{}:{line}: expected `key = value`",
            path.display(),
        );
    }
    for (line, error) in &state.chrome_errors {
        log::warn!(
            target: "zz_gtk::config",
            "{}:{line}: ignoring chrome override: {error}",
            path.display(),
        );
    }
    for host in state.rejected_hosts() {
        log::warn!(
            target: "zz_gtk::config",
            "ignoring `host-{}`: {}",
            host.name,
            host.reason,
        );
    }
    state.path = Some(path.to_owned());
    state
}

/// Write one key, or delete its line when `value` is `None`. The caller then
/// polls: nothing applies from here.
pub fn write(key: &str, value: Option<&str>) -> io::Result<()> {
    let value = value.map(|value| value_for_write(key, value)).transpose()?;
    write_value_at(&file::path_for_write()?, key, value.as_deref())
}

fn write_value_at(path: &Path, key: &str, value: Option<&str>) -> io::Result<()> {
    let Some(value) = value else {
        return file::remove_key_group_at(path, key);
    };
    if key != "font-family" {
        return file::set_key_at(path, key, value);
    }
    if value.bytes().any(|byte| matches!(byte, b'\r' | b'\n')) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Configuration values must fit on one line",
        ));
    }
    let source = file::read_editor_source(path, file::MAX_CONFIG_BYTES)?;
    let edited = file::replace_key_group(&source, key, &[value.to_owned()]);
    if edited == source {
        return Ok(());
    }
    file::write_editor_source(path, &edited, file::MAX_CONFIG_BYTES)
}

fn value_for_write(key: &str, value: &str) -> io::Result<String> {
    let Some(setting) = schema::SETTINGS
        .iter()
        .find(|setting| setting.key == key && setting.owner == Owner::Client)
    else {
        return Ok(value.to_owned());
    };
    let value = normalize_client_value(setting, value).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid value for {key}"),
        )
    })?;
    if matches!(key, "ui-font-family" | "agent-working-directory") {
        let decoded = parse_config_string(&value).expect("validated client string");
        Ok(format!(
            "\"{}\"",
            decoded.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    } else {
        Ok(value)
    }
}

/// Add or remove one `host-<name>` line, through the same comment-preserving
/// writer every other key goes through. A removal takes every duplicate with
/// it — leaving an earlier one behind would only bring the host back — and an
/// addition is refused by the daemon's validator before it reaches the disk.
///
/// Nothing here applies anything: the poll is still what tells the fleet.
pub fn write_host(name: &str, endpoint: Option<&str>) -> io::Result<()> {
    let key = format!("host-{name}");
    let Some(endpoint) = endpoint else {
        return file::remove_key_group(&key);
    };
    validate_fleet_host(name, endpoint)
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    file::set_key(&key, endpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_selection_and_reset_replace_all_prior_overrides() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("zz-gtk-font-reset-{}-{unique}", std::process::id()));
        let original = "# keep this comment\nfont-family = First\nfont-size = 17\nfont-family = Fallback\nstatus-clock = off\nstatus-clock = 12-hour\n";
        std::fs::write(&path, original).unwrap();
        write_value_at(&path, "font-family", Some("Chosen Font")).unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        assert_eq!(source.matches("font-family =").count(), 1);
        assert!(source.starts_with("# keep this comment\nfont-size = 17\n"));
        assert_eq!(
            zz_terminal::load_ghostty_appearance_from(&path)
                .appearance
                .font_families,
            ["Chosen Font"]
        );
        assert!(write_value_at(&path, "font-family", Some("bad\nfont-size = 90")).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        std::fs::write(&path, original).unwrap();
        write_value_at(&path, "font-family", None).unwrap();
        write_value_at(&path, "status-clock", None).unwrap();
        let reset = std::fs::read_to_string(&path).unwrap();
        assert_eq!(reset, "# keep this comment\nfont-size = 17\n");
        assert_eq!(
            zz_terminal::load_ghostty_appearance_from(&path)
                .appearance
                .font_families,
            zz_terminal::TerminalAppearance::default().font_families
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn client_writes_validate_and_quote_before_touching_the_file() {
        assert!(write("agent-working-directory", Some("relative/path")).is_err());
        assert!(write("browser-search-provider", Some("unknown")).is_err());
        assert!(write("ui-font-family", Some("invalid\tfont")).is_err());
        let value =
            value_for_write("agent-working-directory", "/tmp/project #2").expect("valid path");
        let state = parse(&format!("agent-working-directory = {value}\n"));
        assert_eq!(
            state.client_settings().agent_directory,
            Some(PathBuf::from("/tmp/project #2"))
        );
    }

    #[test]
    fn client_values_match_shared_spellings_and_keep_valid_duplicates() {
        let state = parse(
            "quit-daemon-on-exit = YES\nquit-daemon-on-exit = invalid\nstatus-badges = OFF\nbrowser-egress = 0\nbrowser-search-provider = brave\nbrowser-search-provider = unknown\nstatus-align = center\nstatus-clock = time-date\n",
        );
        assert!(state.boolean("quit-daemon-on-exit", false));
        assert_eq!(state.value("quit-daemon-on-exit"), Some("true"));
        let config = state.client_settings();
        assert!(!config.status.badges);
        assert!(!config.browser_egress);
        assert_eq!(config.browser_search, zz_browser::SearchProvider::Brave);
        assert_eq!(config.status.alignment, StatusBarAlignment::Center);
        assert_eq!(config.status.clock, StatusBarClock::TimeAndDate);
    }

    #[test]
    fn quoted_agent_directory_and_interface_font_round_trip() {
        let state = parse(
            "agent-working-directory = '/tmp/my project'\nagent-working-directory = relative/path\nui-font-family = \"Adwaita Sans\"\nanimations = No\n",
        );
        let config = state.client_settings();
        assert_eq!(
            config.agent_directory,
            Some(PathBuf::from("/tmp/my project"))
        );
        assert_eq!(config.ui_font_family.as_deref(), Some("Adwaita Sans"));
        assert!(!config.animations);
        assert!(
            !state
                .daemon_entries()
                .iter()
                .any(|(key, _)| key == "agent-working-directory")
        );
    }

    #[test]
    fn publishing_a_reset_restores_client_defaults() {
        publish(&parse(
            "browser-search-provider = duckduckgo\nstatus-clock = off\n",
        ));
        assert_eq!(
            current().browser_search,
            zz_browser::SearchProvider::DuckDuckGo
        );
        publish(&State::default());
        assert_eq!(current().browser_search, zz_browser::SearchProvider::Google);
        assert_eq!(
            current().status,
            StatusBarSettings {
                show_update: false,
                ..StatusBarSettings::default()
            }
        );
    }

    const SAMPLE: &str = "\
# the zz configuration
theme-mode = dark

pane-margin = 8   # a trailing comment
background = #112233
palette = 0=#000000
palette = 1=#111111
prefix = C-a
";

    #[test]
    fn comments_and_blank_lines_are_not_values() {
        let state = parse(SAMPLE);

        assert_eq!(state.value("theme-mode"), Some("dark"));
        assert!(state.malformed_lines().is_empty());
        assert!(!state.is_overridden("# the zz configuration"));
    }

    #[test]
    fn a_trailing_comment_is_not_part_of_the_value() {
        let state = parse(SAMPLE);

        assert_eq!(state.value("pane-margin"), Some("8"));
        assert_eq!(state.number("pane-margin", 6.0), 8.0);
    }

    #[test]
    fn a_leading_hash_is_a_color_rather_than_a_comment() {
        let state = parse(SAMPLE);

        assert_eq!(state.value("background"), Some("#112233"));
    }

    #[test]
    fn daemon_entries_keep_file_order_and_duplicates() {
        let state = parse(SAMPLE);

        assert_eq!(
            state.daemon_entries(),
            [
                ("background".to_owned(), "#112233".to_owned()),
                ("palette".to_owned(), "0=#000000".to_owned()),
                ("palette".to_owned(), "1=#111111".to_owned()),
                ("prefix".to_owned(), "C-a".to_owned()),
            ]
        );
    }

    #[test]
    fn client_keys_never_reach_the_daemon_vector() {
        let state = parse(SAMPLE);

        assert!(
            !state
                .daemon_entries()
                .iter()
                .any(|(key, _)| key == "theme-mode" || key == "pane-margin")
        );
    }

    #[test]
    fn the_last_occurrence_of_a_key_is_the_effective_one() {
        let state = parse("pane-gaps = false\npane-gaps = true\n");

        assert!(state.boolean("pane-gaps", false));
    }

    #[test]
    fn chrome_overrides_apply_in_file_order() {
        let state = parse(
            "chrome-keybind = ui:C-p=ui-zoom-in\n\
             chrome-unbind = ui:C-p\n\
             chrome-keybind = ui:C-p=open-settings\n\
             chrome-unbind = ui:C-,\n\
             chrome-keybind = browser:D-==browser-zoom-in\n",
        );
        let chrome = state.chrome_keymap();

        assert_eq!(
            chrome.action_for("ui", "C-p"),
            Some(ChromeAction::OpenSettings)
        );
        assert_eq!(chrome.action_for("ui", "C-,"), None);
        assert_eq!(
            chrome.action_for("browser", "D-="),
            Some(ChromeAction::BrowserZoomIn)
        );
    }

    #[test]
    fn invalid_chrome_overrides_are_reported_and_skipped() {
        let state = parse(
            "chrome-keybind = prefix:C-p=open-settings\n\
             chrome-keybind = ui:C-p=teleport\n\
             chrome-unbind = ui:\n\
             chrome-keybind = ui:C-p=open-settings\n",
        );

        assert_eq!(
            state
                .chrome_errors
                .iter()
                .map(|(line, _)| *line)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(
            state.chrome_keymap().action_for("ui", "C-p"),
            Some(ChromeAction::OpenSettings)
        );
    }

    #[test]
    fn provenance_is_presence_rather_than_validity() {
        let state = parse("pane-margin = not-a-number\n");

        assert!(state.is_overridden("pane-margin"));
        assert_eq!(state.number("pane-margin", 6.0), 6.0);
    }

    #[test]
    fn an_absent_key_is_a_default() {
        let state = parse("");

        assert!(!state.is_overridden("pane-margin"));
        assert_eq!(state.number("pane-margin", 6.0), 6.0);
    }

    #[test]
    fn a_line_without_an_equals_sign_is_reported_by_line_number() {
        let state = parse("ok = 1\nbroken\n");

        assert_eq!(state.malformed_lines(), [2]);
    }

    /// The whole apply path, minus the widget that pulls the trigger: a GUI
    /// edit writes the file and the poll is what applies it, so a hand edit and
    /// a click are indistinguishable by construction.
    #[test]
    fn a_written_key_reaches_the_ui_only_through_the_poll() {
        let scratch =
            std::env::temp_dir().join(format!("zz-gtk-store-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&scratch);
        let path = scratch.join("zz/config");
        file::atomic_write(&path, SAMPLE.as_bytes()).expect("seed the config");

        let mut store = Store::for_candidates(vec![path.clone()]);
        assert_eq!(store.state().value("pane-margin"), Some("8"));
        assert!(!store.state().is_overridden("pane-gaps"));

        write_at(&path, "pane-gaps", Some("true"));
        store.invalidate();
        assert!(store.poll(), "the poll must notice the write");
        assert!(store.state().boolean("pane-gaps", false));
        assert!(store.state().is_overridden("pane-gaps"));

        write_at(&path, "pane-margin", None);
        store.invalidate();
        assert!(store.poll());
        assert!(!store.state().is_overridden("pane-margin"));
        assert_eq!(store.state().number("pane-margin", 6.0), 6.0);

        let left = std::fs::read_to_string(&path).expect("read back");
        assert!(
            left.starts_with("# the zz configuration\n"),
            "the writer ate a comment: {left:?}"
        );
        assert!(
            left.contains("prefix = C-a"),
            "the writer ate an unrelated key: {left:?}"
        );
        assert!(!store.poll(), "an unchanged file must not re-apply");

        let _ = std::fs::remove_dir_all(&scratch);
    }

    fn write_at(path: &std::path::Path, key: &str, value: Option<&str>) {
        match value {
            Some(value) => file::set_key_at(path, key, value).expect("set"),
            None => file::remove_key_at(path, key).expect("remove"),
        }
    }

    /// Host lines are the desktop's, resolved by the daemon's own validator:
    /// file order, last duplicate wins, and a bad one is reported rather than
    /// silently dropped.
    #[test]
    fn fleet_hosts_parse_in_config_order_and_report_what_they_reject() {
        let state = parse(
            "host-desktop = ssh://fabrico@desktop:2222\n\
             background = #101010\n\
             host-scratch = unix:///tmp/zz-scratch.sock\n\
             host-desktop = ssh://new-desktop\n\
             host-local = ssh://reserved\n\
             host-broken = quic://gpu:7777\n",
        );

        let hosts: Vec<(&str, String)> = state
            .fleet_hosts()
            .iter()
            .map(|host| (host.name.as_str(), host.endpoint.to_string()))
            .collect();
        assert_eq!(
            hosts,
            [
                ("scratch", "/tmp/zz-scratch.sock".to_owned()),
                ("desktop", "ssh://new-desktop".to_owned()),
            ],
            "a repeated host keeps the last entry, in that entry's position"
        );
        let rejected: Vec<&str> = state
            .rejected_hosts()
            .iter()
            .map(|host| host.name.as_str())
            .collect();
        assert_eq!(rejected, ["local", "broken"]);
        assert!(
            !state
                .daemon_entries()
                .iter()
                .any(|(key, _)| key.starts_with("host-")),
            "the fleet is the client's business; the daemon is never told about it"
        );
    }

    /// Adding and closing a host go through the same comment-preserving writer
    /// every other key uses — and closing takes every duplicate with it, or the
    /// host would come back on the next poll.
    #[test]
    fn a_host_is_written_and_removed_without_disturbing_the_rest_of_the_file() {
        let scratch =
            std::env::temp_dir().join(format!("zz-gtk-hosts-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&scratch);
        let path = scratch.join("zz/config");
        file::atomic_write(
            &path,
            b"# keep\nhost-desktop = ssh://old\nshow-fps = true\nhost-desktop = ssh://new # why\n",
        )
        .expect("seed the config");

        file::set_key_at(&path, "host-gpu", "ssh://gpu:9922").expect("write a host");
        let state = parse(&std::fs::read_to_string(&path).expect("read back"));
        assert_eq!(
            state
                .fleet_hosts()
                .iter()
                .map(|host| host.name.as_str())
                .collect::<Vec<_>>(),
            ["desktop", "gpu"]
        );

        file::remove_key_group_at(&path, "host-desktop").expect("close a host");
        let left = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(left, "# keep\nshow-fps = true\nhost-gpu = ssh://gpu:9922\n");
        assert_eq!(parse(&left).fleet_hosts().len(), 1);

        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn the_experimental_gates_are_daemon_owned_despite_looking_client_local() {
        let state = parse("experimental-agent-pane = true\n");

        assert_eq!(
            state.daemon_entries(),
            [("experimental-agent-pane".to_owned(), "true".to_owned())]
        );
    }
}
