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

    /// Whether resetting can change anything. Only a line in `zz/config` is
    /// this client's to delete; a value the daemon sourced elsewhere is not.
    pub const fn is_resettable(self) -> bool {
        matches!(self, Self::Override)
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
        let candidates = file::candidates();
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

    #[test]
    fn the_experimental_gates_are_daemon_owned_despite_looking_client_local() {
        let state = parse("experimental-agent-pane = true\n");

        assert_eq!(
            state.daemon_entries(),
            [("experimental-agent-pane".to_owned(), "true".to_owned())]
        );
    }
}
