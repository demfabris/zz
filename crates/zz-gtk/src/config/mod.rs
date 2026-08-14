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
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};

use zz_daemon::{HostEntry, RejectedHost, apply_fleet_host_entry, validate_fleet_host};
use zz_protocol::{ConfigOverrideEntry, MuxOptionKey};
use zz_terminal::AppearanceConfigKey;

pub use file::{MAX_CONFIG_BYTES, POLL_INTERVAL};
pub use schema::{Kind, Owner, Page, Setting, Support};

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
    pub fn boolean(&self, key: &str, default: bool) -> bool {
        match self.value(key) {
            Some("true") => true,
            Some("false") => false,
            _ => default,
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
        state.values.insert(key.to_owned(), value.to_owned());
    }
    state
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
    match value {
        Some(value) => file::set_key(key, value),
        None => file::remove_key(key),
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
