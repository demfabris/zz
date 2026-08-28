use std::{
    collections::BTreeMap,
    fmt::{self, Write as _},
    ops::Deref,
    time::{Duration, Instant},
};

use smallvec::SmallVec;
use zz_terminal::{KeyCode, KeyInput};

use crate::message::{CommandInvocation, KeyBindingSnapshot, KeyTableSnapshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub commands: Vec<CommandInvocation>,
    pub repeat: bool,
    pub note: Option<String>,
}

impl Binding {
    /// The default `<prefix> <prefix>` binding that delivers a literal prefix.
    #[must_use]
    pub fn send_prefix() -> Self {
        Self {
            commands: vec![CommandInvocation::new("send-prefix", [] as [&str; 0])],
            repeat: false,
            note: Some("Send the prefix key".to_owned()),
        }
    }

    /// Whether this is exactly the stock `send-prefix` binding.
    #[must_use]
    pub fn is_send_prefix(&self) -> bool {
        matches!(self.commands.as_slice(), [command]
            if command.name == "send-prefix" && command.args.is_empty())
    }
}

#[derive(Debug)]
pub struct KeyTables {
    prefix: String,
    prefix2: Option<String>,
    tables: BTreeMap<String, BTreeMap<String, Binding>>,
}

impl Default for KeyTables {
    fn default() -> Self {
        let mut tables = Self {
            prefix: "C-b".to_owned(),
            prefix2: None,
            tables: BTreeMap::new(),
        };
        for (key, command) in [
            ("c", "new-window"),
            ("%", "split-picker -h"),
            ("\"", "split-picker -v"),
            ("!", "break-pane"),
            ("x", "kill-pane"),
            ("&", "kill-window"),
            ("n", "next-window"),
            ("p", "previous-window"),
            ("l", "last-window"),
            ("]", "paste-buffer"),
            ("o", "select-pane -t:.+"),
            ("C-o", "rotate-window"),
            ("M-o", "rotate-window -D"),
            ("Space", "next-layout"),
            ("E", "select-layout -E"),
            ("M-1", "select-layout even-horizontal"),
            ("M-2", "select-layout even-vertical"),
            ("M-3", "select-layout main-horizontal"),
            ("M-4", "select-layout main-vertical"),
            ("M-5", "select-layout tiled"),
            ("M-6", "select-layout main-horizontal-mirrored"),
            ("M-7", "select-layout main-vertical-mirrored"),
            ("[", "copy-mode"),
            ("?", "list-keys"),
            ("=", "choose-buffer -Z"),
            ("e", "send-last-output"),
            ("s", "focus-sidebar"),
            ("w", "focus-sidebar"),
            ("q", "display-panes"),
            ("r", "reload-config"),
            ("z", "resize-pane -Z"),
            (";", "last-pane"),
            ("{", "swap-pane -U"),
            ("}", "swap-pane -D"),
            (":", "command-prompt"),
        ] {
            let mut parts = command.split_whitespace();
            let name = parts.next().expect("default command has a name");
            tables.bind(
                "prefix",
                key,
                Binding {
                    commands: vec![CommandInvocation::new(name, parts)],
                    repeat: false,
                    note: None,
                },
            );
        }
        for (key, input, template, note) in [
            (
                "$",
                "#S",
                "rename-session -- '%%'",
                "Rename current session",
            ),
            (",", "#W", "rename-window -- '%%'", "Rename current window"),
        ] {
            tables.bind(
                "prefix",
                key,
                Binding {
                    commands: vec![CommandInvocation::new(
                        "command-prompt",
                        ["-I", input, template],
                    )],
                    repeat: false,
                    note: Some(note.to_owned()),
                },
            );
        }
        for digit in 0..=9_u32 {
            tables.bind(
                "prefix",
                &digit.to_string(),
                Binding {
                    commands: vec![CommandInvocation::new(
                        "select-window",
                        ["-t".to_owned(), format!(":{digit}")],
                    )],
                    repeat: false,
                    note: None,
                },
            );
        }
        for (key, flag, note) in [
            ("Up", "-U", "Select the pane above"),
            ("Down", "-D", "Select the pane below"),
            ("Left", "-L", "Select the pane to the left"),
            ("Right", "-R", "Select the pane to the right"),
        ] {
            tables.bind(
                "prefix",
                key,
                Binding {
                    commands: vec![CommandInvocation::new("select-pane", [flag])],
                    repeat: true,
                    note: Some(note.to_owned()),
                },
            );
        }
        for (key, flag, cells, note) in [
            ("M-Up", "-U", "5", "Resize the pane up by 5"),
            ("M-Down", "-D", "5", "Resize the pane down by 5"),
            ("M-Left", "-L", "5", "Resize the pane left by 5"),
            ("M-Right", "-R", "5", "Resize the pane right by 5"),
            ("C-Up", "-U", "1", "Resize the pane up"),
            ("C-Down", "-D", "1", "Resize the pane down"),
            ("C-Left", "-L", "1", "Resize the pane left"),
            ("C-Right", "-R", "1", "Resize the pane right"),
        ] {
            tables.bind(
                "prefix",
                key,
                Binding {
                    commands: vec![CommandInvocation::new("resize-pane", [flag, cells])],
                    repeat: true,
                    note: Some(note.to_owned()),
                },
            );
        }
        for (table, key, action) in [
            ("copy-mode-vi", "h", "cursor-left"),
            ("copy-mode-vi", "Left", "cursor-left"),
            ("copy-mode-vi", "C-h", "cursor-left"),
            ("copy-mode-vi", "BSpace", "cursor-left"),
            ("copy-mode-vi", "l", "cursor-right"),
            ("copy-mode-vi", "Right", "cursor-right"),
            ("copy-mode-vi", "k", "cursor-up"),
            ("copy-mode-vi", "Up", "cursor-up"),
            ("copy-mode-vi", "j", "cursor-down"),
            ("copy-mode-vi", "Down", "cursor-down"),
            ("copy-mode-vi", "0", "start-of-line"),
            ("copy-mode-vi", "Home", "start-of-line"),
            ("copy-mode-vi", "$", "end-of-line"),
            ("copy-mode-vi", "End", "end-of-line"),
            ("copy-mode-vi", "w", "next-word"),
            ("copy-mode-vi", "b", "previous-word"),
            ("copy-mode-vi", "e", "next-word-end"),
            ("copy-mode-vi", "W", "next-space"),
            ("copy-mode-vi", "B", "previous-space"),
            ("copy-mode-vi", "E", "next-space-end"),
            ("copy-mode-vi", "C-u", "halfpage-up"),
            ("copy-mode-vi", "C-b", "page-up"),
            ("copy-mode-vi", "PPage", "page-up"),
            ("copy-mode-vi", "C-d", "halfpage-down"),
            ("copy-mode-vi", "C-f", "page-down"),
            ("copy-mode-vi", "NPage", "page-down"),
            ("copy-mode-vi", "C-y", "scroll-up"),
            ("copy-mode-vi", "K", "scroll-up"),
            ("copy-mode-vi", "C-Up", "scroll-up"),
            ("copy-mode-vi", "C-e", "scroll-down"),
            ("copy-mode-vi", "J", "scroll-down"),
            ("copy-mode-vi", "C-Down", "scroll-down"),
            ("copy-mode-vi", "g", "history-top"),
            ("copy-mode-vi", "G", "history-bottom"),
            ("copy-mode-vi", "^", "back-to-indentation"),
            ("copy-mode-vi", "H", "top-line"),
            ("copy-mode-vi", "M", "middle-line"),
            ("copy-mode-vi", "L", "bottom-line"),
            ("copy-mode-vi", "z", "scroll-middle"),
            ("copy-mode-vi", "{", "previous-paragraph"),
            ("copy-mode-vi", "}", "next-paragraph"),
            ("copy-mode-vi", "%", "next-matching-bracket"),
            ("copy-mode-vi", "Space", "begin-selection"),
            ("copy-mode-vi", "V", "select-line"),
            ("copy-mode-vi", "o", "other-end"),
            ("copy-mode-vi", "v", "rectangle-toggle"),
            ("copy-mode-vi", "C-v", "rectangle-toggle"),
            ("copy-mode-vi", "C-[", "clear-selection"),
            ("copy-mode-vi", "X", "set-mark"),
            ("copy-mode-vi", "M-x", "jump-to-mark"),
            ("copy-mode-vi", "f", "jump-forward"),
            ("copy-mode-vi", "F", "jump-backward"),
            ("copy-mode-vi", "t", "jump-to-forward"),
            ("copy-mode-vi", "T", "jump-to-backward"),
            ("copy-mode-vi", ";", "jump-again"),
            ("copy-mode-vi", ",", "jump-reverse"),
            ("copy-mode-vi", "A", "append-selection-and-cancel"),
            ("copy-mode-vi", "n", "search-again"),
            ("copy-mode-vi", "N", "search-reverse"),
            ("copy-mode-vi", "Enter", "copy-pipe-and-cancel"),
            ("copy-mode-vi", "C-j", "copy-pipe-and-cancel"),
            ("copy-mode-vi", "D", "copy-pipe-end-of-line-and-cancel"),
            ("copy-mode-vi", "q", "cancel"),
            ("copy-mode-vi", "C-c", "cancel"),
            ("copy-mode-vi", "Escape", "clear-selection"),
            ("copy-mode", "Left", "cursor-left"),
            ("copy-mode", "C-b", "cursor-left"),
            ("copy-mode", "Right", "cursor-right"),
            ("copy-mode", "C-f", "cursor-right"),
            ("copy-mode", "Up", "cursor-up"),
            ("copy-mode", "C-p", "cursor-up"),
            ("copy-mode", "Down", "cursor-down"),
            ("copy-mode", "C-n", "cursor-down"),
            ("copy-mode", "C-a", "start-of-line"),
            ("copy-mode", "C-e", "end-of-line"),
            ("copy-mode", "Home", "start-of-line"),
            ("copy-mode", "End", "end-of-line"),
            ("copy-mode", "M-f", "next-word-end"),
            ("copy-mode", "M-b", "previous-word"),
            ("copy-mode", "C-M-f", "next-matching-bracket"),
            ("copy-mode", "M-v", "page-up"),
            ("copy-mode", "PPage", "page-up"),
            ("copy-mode", "C-v", "page-down"),
            ("copy-mode", "NPage", "page-down"),
            ("copy-mode", "Space", "page-down"),
            ("copy-mode", "M-Up", "halfpage-up"),
            ("copy-mode", "M-Down", "halfpage-down"),
            ("copy-mode", "C-Up", "scroll-up"),
            ("copy-mode", "C-Down", "scroll-down"),
            ("copy-mode", "M-<", "history-top"),
            ("copy-mode", "M->", "history-bottom"),
            ("copy-mode", "M-R", "top-line"),
            ("copy-mode", "C-M-Up", "previous-prompt"),
            ("copy-mode", "C-M-Down", "next-prompt"),
            ("copy-mode", "C-Space", "begin-selection"),
            ("copy-mode", "R", "rectangle-toggle"),
            ("copy-mode", "M-m", "back-to-indentation"),
            ("copy-mode", "M-r", "middle-line"),
            ("copy-mode", "M-{", "previous-paragraph"),
            ("copy-mode", "M-}", "next-paragraph"),
            ("copy-mode", "X", "set-mark"),
            ("copy-mode", "M-x", "jump-to-mark"),
            ("copy-mode", "f", "jump-forward"),
            ("copy-mode", "F", "jump-backward"),
            ("copy-mode", "t", "jump-to-forward"),
            ("copy-mode", "T", "jump-to-backward"),
            ("copy-mode", ";", "jump-again"),
            ("copy-mode", ",", "jump-reverse"),
            ("copy-mode", "n", "search-again"),
            ("copy-mode", "N", "search-reverse"),
            ("copy-mode", "Enter", "copy-pipe-and-cancel"),
            ("copy-mode", "M-w", "copy-pipe-and-cancel"),
            ("copy-mode", "C-w", "copy-pipe-and-cancel"),
            ("copy-mode", "C-k", "copy-pipe-end-of-line-and-cancel"),
            ("copy-mode", "q", "cancel"),
            ("copy-mode", "C-g", "clear-selection"),
            ("copy-mode", "C-c", "cancel"),
            ("copy-mode", "C-[", "cancel"),
            ("copy-mode", "Escape", "cancel"),
            ("choose-tree", "Up", "cursor-up"),
            ("choose-tree", "k", "cursor-up"),
            ("choose-tree", "C-p", "cursor-up"),
            ("choose-tree", "Down", "cursor-down"),
            ("choose-tree", "j", "cursor-down"),
            ("choose-tree", "C-n", "cursor-down"),
            ("choose-tree", "PPage", "page-up"),
            ("choose-tree", "C-b", "page-up"),
            ("choose-tree", "NPage", "page-down"),
            ("choose-tree", "C-f", "page-down"),
            ("choose-tree", "Home", "history-top"),
            ("choose-tree", "g", "history-top"),
            ("choose-tree", "End", "history-bottom"),
            ("choose-tree", "G", "history-bottom"),
            ("choose-tree", "Left", "collapse"),
            ("choose-tree", "h", "collapse"),
            ("choose-tree", "-", "collapse"),
            ("choose-tree", "Right", "expand"),
            ("choose-tree", "l", "expand"),
            ("choose-tree", "+", "expand"),
            ("choose-tree", "Enter", "accept"),
            ("choose-tree", "q", "cancel"),
            ("choose-tree", "Escape", "cancel"),
            ("choose-tree", "C-g", "cancel"),
            ("choose-tree", "C-[", "cancel"),
            ("choose-tree", "/", "search-forward"),
            ("choose-tree", "C-s", "search-forward"),
            ("choose-tree", "?", "search-backward"),
            ("choose-tree", "n", "search-again"),
            ("choose-tree", "N", "search-reverse"),
            ("choose-buffer", "Up", "cursor-up"),
            ("choose-buffer", "k", "cursor-up"),
            ("choose-buffer", "C-p", "cursor-up"),
            ("choose-buffer", "Down", "cursor-down"),
            ("choose-buffer", "j", "cursor-down"),
            ("choose-buffer", "C-n", "cursor-down"),
            ("choose-buffer", "PPage", "page-up"),
            ("choose-buffer", "C-b", "page-up"),
            ("choose-buffer", "NPage", "page-down"),
            ("choose-buffer", "C-f", "page-down"),
            ("choose-buffer", "Home", "history-top"),
            ("choose-buffer", "g", "history-top"),
            ("choose-buffer", "End", "history-bottom"),
            ("choose-buffer", "G", "history-bottom"),
            ("choose-buffer", "Enter", "accept"),
            ("choose-buffer", "p", "paste"),
            ("choose-buffer", "d", "delete"),
            ("choose-buffer", "q", "cancel"),
            ("choose-buffer", "Escape", "cancel"),
            ("choose-buffer", "C-g", "cancel"),
            ("choose-buffer", "C-[", "cancel"),
            ("choose-buffer", "/", "search-forward"),
            ("choose-buffer", "C-s", "search-forward"),
            ("choose-buffer", "?", "search-backward"),
            ("choose-buffer", "n", "search-again"),
            ("choose-buffer", "N", "search-reverse"),
        ] {
            tables.bind(
                table,
                key,
                Binding {
                    commands: vec![CommandInvocation::new("send-keys", ["-X", action])],
                    repeat: !matches!(table, "copy-mode" | "copy-mode-vi"),
                    note: None,
                },
            );
        }
        for (key, action) in [
            ("#", "search-backward-cursor-word"),
            ("*", "search-forward-cursor-word"),
        ] {
            tables.bind(
                "copy-mode-vi",
                key,
                Binding {
                    commands: vec![CommandInvocation::new("send-keys", ["-X", action])],
                    repeat: false,
                    note: None,
                },
            );
        }
        for digit in '1'..='9' {
            tables.bind(
                "copy-mode-vi",
                &digit.to_string(),
                Binding {
                    commands: vec![CommandInvocation::new(
                        "copy-mode-repeat",
                        [digit.to_string()],
                    )],
                    repeat: false,
                    note: None,
                },
            );
        }
        tables.bind(
            "copy-mode-vi",
            ":",
            Binding {
                commands: vec![CommandInvocation::new(
                    "command-prompt",
                    ["-p", "(goto line)", "send-keys -X goto-line -- '%%'"],
                )],
                repeat: false,
                note: None,
            },
        );
        tables.bind_copy_mode_search_defaults();
        // tmux's stock `bind C-b send-prefix`.
        let prefix = tables.prefix.clone();
        tables.bind("prefix", &prefix, Binding::send_prefix());
        tables
    }
}

impl KeyTables {
    /// Tables with no bindings at all, for key surfaces that seed their own
    /// defaults (client-local chrome tables) instead of the tmux set.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            prefix: "C-b".to_owned(),
            prefix2: None,
            tables: BTreeMap::new(),
        }
    }

    fn bind_copy_mode_search_defaults(&mut self) {
        for (table, key, backward) in [
            ("copy-mode-vi", "/", false),
            ("copy-mode-vi", "?", true),
            ("copy-mode", "C-s", false),
            ("copy-mode", "C-r", true),
        ] {
            let arguments = backward.then_some("-b");
            self.bind(
                table,
                key,
                Binding {
                    commands: vec![CommandInvocation::new("copy-mode-search-prompt", arguments)],
                    repeat: false,
                    note: None,
                },
            );
        }
    }

    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    #[must_use]
    pub fn prefix2(&self) -> Option<&str> {
        self.prefix2.as_deref()
    }

    /// Set or clear the secondary prefix. The pin's default bindings carry no
    /// `send-prefix -2` entry, so unlike [`Self::set_prefix`] this never
    /// touches the tables.
    pub fn set_prefix2(&mut self, prefix2: Option<&str>) {
        self.prefix2 = prefix2
            .filter(|key| !key.eq_ignore_ascii_case("none"))
            .map(canonical_key);
    }

    /// Whether a canonical key arms the prefix table: the prefix or, when
    /// set, the secondary prefix.
    #[must_use]
    pub fn is_prefix(&self, key: &str) -> bool {
        key == self.prefix || self.prefix2.as_deref() == Some(key)
    }

    /// Change the effective prefix, carrying a stock `send-prefix` binding with
    /// it. A binding the user customized stays on the old key.
    pub fn set_prefix(&mut self, prefix: impl Into<String>) {
        let next = canonical_key(&prefix.into());
        let previous = std::mem::replace(&mut self.prefix, next);
        if previous == self.prefix {
            return;
        }
        if self
            .get("prefix", &previous)
            .is_some_and(Binding::is_send_prefix)
        {
            self.unbind("prefix", &previous);
            let next = self.prefix.clone();
            if self.get("prefix", &next).is_none() {
                self.bind("prefix", &next, Binding::send_prefix());
            }
        }
    }

    pub fn bind(&mut self, table: &str, key: &str, binding: Binding) {
        self.tables
            .entry(table.to_owned())
            .or_default()
            .insert(canonical_key(key), binding);
    }

    pub fn unbind(&mut self, table: &str, key: &str) -> bool {
        let removed = self
            .tables
            .get_mut(table)
            .and_then(|bindings| bindings.remove(&canonical_key(key)))
            .is_some();
        if removed && self.tables.get(table).is_some_and(BTreeMap::is_empty) {
            self.tables.remove(table);
        }
        removed
    }

    pub fn remove_table(&mut self, table: &str) -> bool {
        self.tables.remove(table).is_some()
    }

    pub fn ensure_table(&mut self, table: &str) {
        self.tables.entry(table.to_owned()).or_default();
    }

    #[must_use]
    pub fn get(&self, table: &str, key: &str) -> Option<&Binding> {
        self.tables
            .get(table)
            .and_then(|bindings| bindings.get(&canonical_key(key)))
    }

    pub fn list(&self, table: Option<&str>) -> impl Iterator<Item = (&str, &str, &Binding)> {
        self.tables.iter().flat_map(move |(name, bindings)| {
            bindings
                .iter()
                .filter(move |_| table.is_none_or(|wanted| wanted == name))
                .map(move |(key, binding)| (name.as_str(), key.as_str(), binding))
        })
    }

    pub fn table_names(&self) -> impl Iterator<Item = &str> {
        self.tables.keys().map(String::as_str)
    }

    /// The binding a key press resolves to in `table`, preferring the
    /// character the press typed (`?` from shift+`/`) over the folded
    /// physical key name, with an `Any` fallback. This is the one lookup
    /// semantic shared by the daemon's overlay routing and client-local
    /// chrome tables.
    #[must_use]
    pub fn resolve_input(&self, table: &str, input: &KeyInput) -> Option<&Binding> {
        if let Some(text) = input_typed_text(input)
            && text.chars().count() == 1
            && let Some(binding) = self.get(table, text)
        {
            return Some(binding);
        }
        self.get(table, input_key_name(input).as_str())
            .or_else(|| self.get(table, "Any"))
    }

    /// Every table flattened for the wire, with command names canonicalized
    /// so a client matching on `split-window` also catches `splitw`.
    #[must_use]
    pub fn snapshot(&self) -> Vec<KeyTableSnapshot> {
        self.tables
            .iter()
            .map(|(name, bindings)| KeyTableSnapshot {
                name: name.clone(),
                bindings: bindings
                    .iter()
                    .map(|(key, binding)| KeyBindingSnapshot {
                        key: key.clone(),
                        commands: binding
                            .commands
                            .iter()
                            .map(|command| {
                                let mut command = command.clone();
                                command.name =
                                    crate::catalog::canonical_command(&command.name).to_owned();
                                command.source = None;
                                command
                            })
                            .collect(),
                        repeat: binding.repeat,
                        note: binding.note.clone(),
                    })
                    .collect(),
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyDecision {
    Pass,
    Prefix,
    Ignore,
    Commands(Vec<CommandInvocation>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyEngine {
    table: Option<String>,
    pending: Option<(Vec<CommandInvocation>, bool)>,
    repeat_count: Option<CopyModeRepeatPrefix>,
    repeat_deadline: Option<Instant>,
    prefix_deadline: Option<Instant>,
    last_repeat_key: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopyModeRepeatPrefix {
    Armed(u16),
    Capturing(u16),
}

impl CopyModeRepeatPrefix {
    fn count(self) -> u16 {
        match self {
            Self::Armed(count) | Self::Capturing(count) => count,
        }
    }
}

impl KeyEngine {
    #[must_use]
    pub fn active_table(&self) -> Option<&str> {
        self.table.as_deref()
    }

    pub fn set_repeat_count(&mut self, count: u32) {
        self.repeat_count = if count <= 1 {
            None
        } else {
            Some(CopyModeRepeatPrefix::Armed(
                u16::try_from(count.min(9_999)).expect("clamped repeat count fits in u16"),
            ))
        };
    }

    fn take_pending_jump_target(&mut self, key: &str) -> Option<(KeyDecision, bool)> {
        let (mut commands, repeat) = self.pending.take()?;
        if key == "Escape" {
            return Some((KeyDecision::Ignore, false));
        }
        for command in &mut commands {
            if copy_jump_needs_target(command) {
                command.args.push(key.to_owned());
            }
        }
        Some((KeyDecision::Commands(commands), repeat))
    }

    fn decide(
        &mut self,
        mut commands: Vec<CommandInvocation>,
        repeat: bool,
    ) -> (KeyDecision, bool) {
        if let Some(digit) = copy_mode_repeat_digit(&commands) {
            self.repeat_count = Some(CopyModeRepeatPrefix::Capturing(u16::from(digit)));
            return (KeyDecision::Ignore, false);
        }
        let copy_action = commands
            .iter_mut()
            .find(|command| copy_mode_action_command(command));
        if let Some(command) = copy_action {
            let (mode_index, has_repeat) =
                copy_mode_action_options(command).expect("copy-mode action options were found");
            if let Some(prefix) = self.repeat_count.take()
                && !has_repeat
            {
                let count = prefix.count();
                command.args.insert(mode_index, count.to_string());
                command.args.insert(mode_index, "-N".to_owned());
            }
        } else if commands.iter().any(copy_mode_prefix_consuming_prompt) {
            self.repeat_count = None;
        }
        if commands.iter().any(copy_jump_needs_target) {
            self.pending = Some((commands, repeat));
            (KeyDecision::Ignore, false)
        } else {
            (KeyDecision::Commands(commands), repeat)
        }
    }

    fn decide_synthetic_any(
        &mut self,
        commands: Vec<CommandInvocation>,
        repeat: bool,
    ) -> (KeyDecision, bool) {
        let pending = self.pending.take();
        let decision = self.decide(commands, repeat);
        self.pending = pending;
        decision
    }

    pub fn handle(&mut self, tables: &KeyTables, key: &str) -> KeyDecision {
        self.handle_with_repeat_time(tables, key, Instant::now(), Duration::from_millis(500))
    }

    pub fn handle_with_repeat_time(
        &mut self,
        tables: &KeyTables,
        key: &str,
        now: Instant,
        repeat_time: Duration,
    ) -> KeyDecision {
        self.handle_with_repeat_times(
            tables,
            key,
            now,
            repeat_time,
            Duration::ZERO,
            Duration::ZERO,
            "root",
        )
    }

    pub fn handle_with_repeat_times(
        &mut self,
        tables: &KeyTables,
        key: &str,
        now: Instant,
        repeat_time: Duration,
        initial_repeat_time: Duration,
        prefix_timeout: Duration,
        root_table: &str,
    ) -> KeyDecision {
        self.handle_with_repeat_metadata(
            tables,
            key,
            now,
            repeat_time,
            initial_repeat_time,
            prefix_timeout,
            root_table,
        )
        .0
    }

    pub fn handle_with_repeat_metadata(
        &mut self,
        tables: &KeyTables,
        key: &str,
        now: Instant,
        repeat_time: Duration,
        initial_repeat_time: Duration,
        prefix_timeout: Duration,
        root_table: &str,
    ) -> (KeyDecision, bool) {
        let key = canonical_key(key);
        if self.repeat_deadline.is_some_and(|deadline| now >= deadline) {
            self.table = None;
            self.repeat_deadline = None;
            self.prefix_deadline = None;
            self.last_repeat_key = None;
        }
        if let Some(decision) = self.take_pending_jump_target(&key) {
            return decision;
        }
        if self.table.as_deref() == Some("copy-mode-vi")
            && key.len() == 1
            && let Some(digit) = key.as_bytes().first().copied().filter(u8::is_ascii_digit)
            && let Some(CopyModeRepeatPrefix::Capturing(count)) = self.repeat_count
        {
            self.repeat_count = Some(CopyModeRepeatPrefix::Capturing(
                count
                    .saturating_mul(10)
                    .saturating_add(u16::from(digit - b'0'))
                    .min(9_999),
            ));
            return (KeyDecision::Ignore, false);
        }
        if self.table.is_none() && tables.is_prefix(&key) {
            self.table = Some("prefix".to_owned());
            self.repeat_deadline = None;
            self.prefix_deadline = (!prefix_timeout.is_zero()).then_some(now + prefix_timeout);
            self.last_repeat_key = None;
            return (KeyDecision::Prefix, false);
        }
        let mut table = self.table.clone().unwrap_or_else(|| root_table.to_owned());
        let exact_binding = tables.get(&table, &key);
        let prefix_expired = table == "prefix"
            && self.prefix_deadline.is_some_and(|deadline| now > deadline)
            && !(self.repeat_deadline.is_some()
                && exact_binding.is_some_and(|binding| binding.repeat));
        // prefix-timeout expires lazily on the next key; there is no timer to clear the client indicator.
        if prefix_expired {
            self.table = None;
            self.prefix_deadline = None;
            root_table.clone_into(&mut table);
        }
        let (mut binding, mut binding_key) = if let Some(binding) = tables.get(&table, &key) {
            (Some(binding), key.clone())
        } else {
            (tables.get(&table, "Any"), "Any".to_owned())
        };
        if self.repeat_deadline.is_some() && binding.is_none_or(|binding| !binding.repeat) {
            self.table = None;
            self.repeat_deadline = None;
            self.prefix_deadline = None;
            self.last_repeat_key = None;
            if tables.is_prefix(&key) {
                self.table = Some("prefix".to_owned());
                self.prefix_deadline = (!prefix_timeout.is_zero()).then_some(now + prefix_timeout);
                return (KeyDecision::Prefix, false);
            }
            root_table.clone_into(&mut table);
            if let Some(root_binding) = tables.get(root_table, &key) {
                binding = Some(root_binding);
                binding_key.clone_from(&key);
            } else {
                binding = tables.get(root_table, "Any");
                "Any".clone_into(&mut binding_key);
            }
        }
        let Some(binding) = binding else {
            if table == "prefix" {
                self.table = None;
                self.prefix_deadline = None;
                self.last_repeat_key = None;
                return (KeyDecision::Ignore, false);
            }
            return if self.table.is_some() {
                (KeyDecision::Ignore, false)
            } else {
                (KeyDecision::Pass, false)
            };
        };
        if table == "prefix" && binding.repeat && !repeat_time.is_zero() {
            let repeat_time = if self.repeat_deadline.is_none()
                || self.last_repeat_key.as_deref() != Some(binding_key.as_str())
            {
                if initial_repeat_time.is_zero() {
                    repeat_time
                } else {
                    initial_repeat_time
                }
            } else {
                repeat_time
            };
            self.table = Some(table.clone());
            self.repeat_deadline = Some(now + repeat_time);
            self.last_repeat_key = Some(binding_key);
        } else if table == "prefix" {
            self.table = None;
            self.repeat_deadline = None;
            self.prefix_deadline = None;
            self.last_repeat_key = None;
        }
        let commands = binding.commands.clone();
        self.decide(commands, binding.repeat)
    }

    pub fn handle_synthetic_any_with_repeat_metadata(
        &mut self,
        tables: &KeyTables,
        now: Instant,
        repeat_time: Duration,
        initial_repeat_time: Duration,
        root_table: &str,
    ) -> (KeyDecision, bool) {
        if self.repeat_deadline.is_some_and(|deadline| now >= deadline) {
            self.clear_explicit_table();
        }

        let mut table = self.table.clone().unwrap_or_else(|| root_table.to_owned());
        let mut explicit = self.table.is_some() && table != root_table;
        let mut binding = tables.get(&table, "Any");
        let prefix_expired =
            table == "prefix" && self.prefix_deadline.is_some_and(|deadline| now > deadline);
        if prefix_expired
            || (self.repeat_deadline.is_some() && binding.is_none_or(|binding| !binding.repeat))
            || (binding.is_none() && explicit)
        {
            self.clear_explicit_table();
            root_table.clone_into(&mut table);
            explicit = false;
            binding = tables.get(root_table, "Any");
        }

        let Some(binding) = binding else {
            return (KeyDecision::Ignore, false);
        };
        if explicit && binding.repeat && !repeat_time.is_zero() {
            let repeat_time = if self.repeat_deadline.is_none()
                || self.last_repeat_key.as_deref() != Some("Any")
            {
                if initial_repeat_time.is_zero() {
                    repeat_time
                } else {
                    initial_repeat_time
                }
            } else {
                repeat_time
            };
            self.table = Some(table);
            self.repeat_deadline = Some(now + repeat_time);
            self.last_repeat_key = Some("Any".to_owned());
        } else if explicit {
            self.clear_explicit_table();
        }
        self.decide_synthetic_any(binding.commands.clone(), binding.repeat)
    }

    pub fn handle_transient_mode_synthetic_any(
        &mut self,
        tables: &KeyTables,
        mode_table: &str,
        root_table: &str,
    ) -> (KeyDecision, bool) {
        let Some(binding) = tables
            .get(mode_table, "Any")
            .or_else(|| tables.get(root_table, "Any"))
        else {
            return (KeyDecision::Ignore, false);
        };
        self.decide_synthetic_any(binding.commands.clone(), binding.repeat)
    }

    fn clear_explicit_table(&mut self) {
        self.table = None;
        self.repeat_deadline = None;
        self.prefix_deadline = None;
        self.last_repeat_key = None;
    }

    pub fn switch_table(&mut self, table: Option<String>) {
        self.table = table;
        self.pending = None;
        self.repeat_count = None;
        self.repeat_deadline = None;
        self.prefix_deadline = None;
        self.last_repeat_key = None;
    }
}

fn copy_mode_repeat_digit(commands: &[CommandInvocation]) -> Option<u8> {
    let [command] = commands else {
        return None;
    };
    if command.name != "copy-mode-repeat" {
        return None;
    }
    let [digit] = command.args.as_slice() else {
        return None;
    };
    let [digit] = digit.as_bytes() else {
        return None;
    };
    digit.is_ascii_digit().then_some(*digit - b'0')
}

fn copy_mode_action_command(command: &CommandInvocation) -> bool {
    copy_mode_action_options(command).is_some()
}

fn copy_mode_prefix_consuming_prompt(command: &CommandInvocation) -> bool {
    matches!(
        command.name.as_str(),
        "copy-mode-search-prompt" | "command-prompt"
    )
}

fn copy_mode_action_options(command: &CommandInvocation) -> Option<(usize, bool)> {
    if !matches!(command.name.as_str(), "send" | "send-keys") {
        return None;
    }
    let mut mode_index = None;
    let mut has_repeat = false;
    let mut skip_value = false;
    for (index, argument) in command.args.iter().enumerate() {
        if skip_value {
            skip_value = false;
            continue;
        }
        if argument == "--" || !argument.starts_with('-') || argument == "-" {
            break;
        }
        let flags = &argument.as_bytes()[1..];
        for (offset, flag) in flags.iter().copied().enumerate() {
            match flag {
                b'X' => {
                    mode_index.get_or_insert(index);
                }
                b'N' => {
                    has_repeat = true;
                    skip_value = offset + 1 == flags.len();
                    break;
                }
                b't' | b'c' => {
                    skip_value = offset + 1 == flags.len();
                    break;
                }
                _ => {}
            }
        }
    }
    mode_index.map(|index| (index, has_repeat))
}

fn copy_jump_needs_target(command: &CommandInvocation) -> bool {
    if !matches!(command.name.as_str(), "send" | "send-keys") {
        return false;
    }
    let Some(mode_index) = command.args.iter().position(|argument| argument == "-X") else {
        return false;
    };
    let Some(action) = command.args.get(mode_index + 1) else {
        return false;
    };
    matches!(
        action.as_str(),
        "jump-forward" | "jump-backward" | "jump-to-forward" | "jump-to-backward"
    ) && command.args[mode_index + 2..]
        .iter()
        .all(|argument| argument == "--")
}

const INLINE_KEY_NAME_BYTES: usize = 16;

/// A tmux-grammar key name assembled without heap allocation for every
/// common chord.
pub struct KeyName {
    bytes: SmallVec<[u8; INLINE_KEY_NAME_BYTES]>,
}

impl KeyName {
    fn new() -> Self {
        Self {
            bytes: SmallVec::new(),
        }
    }

    fn push_str(&mut self, value: &str) {
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn push_char(&mut self, value: char) {
        let mut encoded = [0_u8; 4];
        self.push_str(value.encode_utf8(&mut encoded));
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes).expect("key names are assembled from valid UTF-8")
    }

    #[must_use]
    pub fn into_string(self) -> String {
        String::from_utf8(self.bytes.into_vec()).expect("key names are assembled from valid UTF-8")
    }

    /// Whether the name outgrew its inline storage; diagnostics only.
    #[must_use]
    pub fn spilled(&self) -> bool {
        self.bytes.spilled()
    }
}

impl fmt::Write for KeyName {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.push_str(value);
        Ok(())
    }
}

impl Deref for KeyName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

fn shifted_character(input: &KeyInput, character: char) -> char {
    if input.modifiers.control() || input.modifiers.alt() {
        return character;
    }
    if input.modifiers.shift() && character.is_ascii_lowercase() {
        return character.to_ascii_uppercase();
    }
    character
}

/// Fold a wire key press into the tmux-grammar name the key tables index by.
#[must_use]
pub fn input_key_name(input: &KeyInput) -> KeyName {
    let mut name = KeyName::new();
    if input.modifiers.platform() {
        return name;
    }
    if input.modifiers.control() {
        name.push_str("C-");
    }
    if input.modifiers.alt() {
        name.push_str("M-");
    }
    match input.key {
        KeyCode::Character(character) => name.push_char(shifted_character(input, character)),
        KeyCode::Backspace => name.push_str("BSpace"),
        KeyCode::Enter => name.push_str("Enter"),
        KeyCode::Tab => name.push_str("Tab"),
        KeyCode::Escape => name.push_str("Escape"),
        KeyCode::Delete => name.push_str("DC"),
        KeyCode::Insert => name.push_str("IC"),
        KeyCode::Home => name.push_str("Home"),
        KeyCode::End => name.push_str("End"),
        KeyCode::PageUp => name.push_str("PPage"),
        KeyCode::PageDown => name.push_str("NPage"),
        KeyCode::ArrowUp => name.push_str("Up"),
        KeyCode::ArrowDown => name.push_str("Down"),
        KeyCode::ArrowLeft => name.push_str("Left"),
        KeyCode::ArrowRight => name.push_str("Right"),
        KeyCode::Function(number) => write!(&mut name, "F{number}").expect("writing key name"),
        KeyCode::Unidentified => name.push_str(input.text.as_deref().unwrap_or_default()),
    }
    name
}

/// Printable text a key press typed, if the chord carries no command
/// modifiers.
#[must_use]
pub fn input_typed_text(input: &KeyInput) -> Option<&str> {
    if input.modifiers.control() || input.modifiers.alt() || input.modifiers.platform() {
        return None;
    }
    let text = input.text.as_deref()?;
    (!text.is_empty() && !text.chars().any(char::is_control)).then_some(text)
}

/// Whether `value` spells a key some [`KeyInput`] can produce, in exactly the
/// grammar [`input_key_name`] emits. Callers that resolve a key from user text
/// — chooser row shortcuts, for one — follow the pin's rule that a spelling
/// nothing can press means no key at all, the way `key_string_lookup_string`
/// answers `KEYC_UNKNOWN` in key-string.c. Canonicalize with [`canonical_key`]
/// first: this accepts only the `C-` before `M-` order the emitter uses.
#[must_use]
pub fn is_key_name(value: &str) -> bool {
    let rest = value.strip_prefix("C-").unwrap_or(value);
    let rest = rest.strip_prefix("M-").unwrap_or(rest);
    let mut characters = rest.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        return !character.is_control();
    }
    if let Some(number) = rest.strip_prefix('F') {
        return matches!(number.parse::<u8>(), Ok(1..=12));
    }
    matches!(
        rest,
        "BSpace"
            | "Enter"
            | "Tab"
            | "Escape"
            | "DC"
            | "IC"
            | "Home"
            | "End"
            | "PPage"
            | "NPage"
            | "Up"
            | "Down"
            | "Left"
            | "Right"
    )
}

/// Fold a tmux key spelling into its canonical form: `Ctrl-`/`Alt-` become
/// `C-`/`M-` and `Space` becomes a literal space, on both sides of any
/// modifier chain.
#[must_use]
pub fn canonical_key(value: &str) -> String {
    let trimmed = value.trim();
    if value == " " || trimmed == "Space" {
        return " ".to_owned();
    }
    // The pin's key_string_table spells the empty key `None` and parses it
    // case-insensitively (key-string.c), so every spelling round-trips as `None`.
    if trimmed.eq_ignore_ascii_case("none") {
        return "None".to_owned();
    }
    let mut control = false;
    let mut alt = false;
    let mut rest = trimmed;
    loop {
        if let Some(tail) = rest
            .strip_prefix("Ctrl-")
            .or_else(|| rest.strip_prefix("C-"))
        {
            control = true;
            rest = tail;
        } else if let Some(tail) = rest
            .strip_prefix("Alt-")
            .or_else(|| rest.strip_prefix("M-"))
        {
            alt = true;
            rest = tail;
        } else {
            break;
        }
    }
    if !control && !alt {
        return trimmed.to_owned();
    }
    if rest == "Space" {
        rest = " ";
    }
    if rest.is_empty() && value.ends_with(' ') {
        rest = " ";
    }
    let mut modifiers = String::new();
    if control {
        modifiers.push_str("C-");
    }
    if alt {
        modifiers.push_str("M-");
    }
    format!("{modifiers}{rest}")
}

#[cfg(test)]
mod tests {
    use zz_terminal::{KeyAction, Modifiers};

    use super::*;

    fn press(key: KeyCode, modifiers: Modifiers, text: Option<&str>) -> KeyInput {
        KeyInput {
            action: KeyAction::Press,
            key,
            modifiers,
            text: text.map(|text| text.to_owned().into_boxed_str()),
            unshifted_codepoint: None,
        }
    }

    #[test]
    fn shifted_letters_fold_to_their_uppercase_name() {
        for letter in ['a', 'g', 'n', 'x'] {
            let upper = letter.to_ascii_uppercase();
            let input = press(
                KeyCode::Character(letter),
                Modifiers::new(true, false, false, false),
                Some(&upper.to_string()),
            );
            assert_eq!(input_key_name(&input).as_str(), upper.to_string());
        }
    }

    #[test]
    fn already_resolved_shifted_symbols_are_left_alone() {
        for symbol in ['#', '*', '%', ':', '?'] {
            let input = press(
                KeyCode::Character(symbol),
                Modifiers::default(),
                Some(&symbol.to_string()),
            );
            assert_eq!(input_key_name(&input).as_str(), symbol.to_string());
        }
    }

    #[test]
    fn a_key_is_spelled_the_same_on_press_and_release() {
        let shift = Modifiers::new(true, false, false, false);
        let mut release = press(KeyCode::Character('g'), shift, None);
        release.action = KeyAction::Release;
        assert_eq!(input_key_name(&release).as_str(), "G");
    }

    #[test]
    fn a_platform_chord_never_names_a_bare_key() {
        let command = Modifiers::new(false, false, false, true);
        let input = press(KeyCode::Character('x'), command, None);
        assert!(input_key_name(&input).as_str().is_empty());
    }

    #[test]
    fn common_tmux_key_names_stay_in_stack_storage() {
        let control_alt = Modifiers::new(false, true, true, false);
        for (input, name) in [
            (press(KeyCode::Enter, Modifiers::default(), None), "Enter"),
            (press(KeyCode::ArrowLeft, control_alt, None), "C-M-Left"),
            (press(KeyCode::Function(255), control_alt, None), "C-M-F255"),
            (press(KeyCode::Character('λ'), control_alt, None), "C-M-λ"),
        ] {
            let rendered = input_key_name(&input);
            assert_eq!(rendered.as_str(), name);
            assert!(!rendered.spilled());
        }
    }

    #[test]
    fn modifier_aliases_canonicalize_in_live_order() {
        for key in ["Alt-Ctrl-x", "M-C-x", "Ctrl-Alt-x", "C-M-x"] {
            assert_eq!(canonical_key(key), "C-M-x");
        }
        assert_eq!(canonical_key("Alt-Ctrl-Space"), "C-M- ");
        assert_eq!(canonical_key("M-C- "), "C-M- ");
    }

    #[test]
    fn inverse_order_modifier_binding_resolves_live_input() {
        let mut tables = KeyTables::default();
        let binding = Binding {
            commands: vec![CommandInvocation::new("display-message", ["matched"])],
            repeat: false,
            note: None,
        };
        tables.bind("root", "Alt-Ctrl-x", binding.clone());
        let input = press(
            KeyCode::Character('x'),
            Modifiers::new(false, true, true, false),
            None,
        );

        assert_eq!(input_key_name(&input).as_str(), "C-M-x");
        assert_eq!(tables.resolve_input("root", &input), Some(&binding));
    }

    #[test]
    fn resolve_input_prefers_the_typed_character() {
        let tables = KeyTables::default();
        let question = press(
            KeyCode::Character('/'),
            Modifiers::new(true, false, false, false),
            Some("?"),
        );
        let binding = tables
            .resolve_input("choose-tree", &question)
            .expect("? resolves in choose-tree");
        assert_eq!(
            binding.commands,
            vec![CommandInvocation::new(
                "send-keys",
                ["-X", "search-backward"]
            )]
        );
        let jay = press(KeyCode::Character('j'), Modifiers::default(), Some("j"));
        assert!(tables.resolve_input("choose-tree", &jay).is_some());
        assert!(tables.resolve_input("no-such-table", &jay).is_none());
    }

    #[test]
    fn published_tables_canonicalize_command_names_and_cover_every_table() {
        let mut tables = KeyTables::default();
        tables.bind(
            "prefix",
            "|",
            Binding {
                commands: vec![
                    CommandInvocation::new("splitw", ["-h", "{ display-message -p true }"])
                        .with_command_blocks([1])
                        .with_source(crate::SourceSpan {
                            source: "test.conf".to_owned(),
                            line: 4,
                            column: 2,
                        }),
                ],
                repeat: false,
                note: Some("Split right".to_owned()),
            },
        );
        tables.bind(
            "root",
            "F1",
            Binding {
                commands: vec![CommandInvocation::new("neww", [] as [&str; 0])],
                repeat: false,
                note: None,
            },
        );
        let published = tables.snapshot();
        let table = |name: &str| {
            published
                .iter()
                .find(|table| table.name == name)
                .unwrap_or_else(|| panic!("the {name} table is published"))
        };
        let pipe = table("prefix")
            .bindings
            .iter()
            .find(|binding| binding.key == "|")
            .expect("the | binding is published");
        assert_eq!(pipe.commands[0].name, "split-window");
        assert_eq!(
            pipe.commands[0].args,
            vec!["-h".to_owned(), "{ display-message -p true }".to_owned()]
        );
        assert!(pipe.commands[0].argument_is_command_block(1));
        assert_eq!(pipe.commands[0].source, None);
        assert_eq!(pipe.note.as_deref(), Some("Split right"));
        let f1 = table("root")
            .bindings
            .iter()
            .find(|binding| binding.key == "F1")
            .expect("the root F1 binding is published");
        assert_eq!(f1.commands[0].name, "new-window");
        for name in ["copy-mode", "copy-mode-vi"] {
            assert!(!table(name).bindings.is_empty(), "{name} bindings publish");
        }
        let repeatable = table("prefix")
            .bindings
            .iter()
            .find(|binding| binding.key == "Left")
            .expect("the arrow select binding is published");
        assert!(repeatable.repeat);
    }

    #[test]
    fn a_space_bearing_key_folds_inside_a_modifier_chord() {
        assert_eq!(canonical_key("C-Space"), "C- ");
        assert_eq!(canonical_key("Ctrl-Space"), "C- ");
        assert_eq!(canonical_key("M-Space"), "M- ");
        assert_eq!(canonical_key("Space"), " ");
        assert_eq!(canonical_key(" "), " ");
        assert_eq!(canonical_key("C- "), "C- ");
        assert_eq!(canonical_key("Ctrl-a"), "C-a");
        assert_eq!(canonical_key("C-b"), "C-b");
        assert_eq!(canonical_key("M-Right"), "M-Right");
        assert_eq!(canonical_key("F2"), "F2");
    }

    #[test]
    fn a_space_bearing_prefix_can_still_send_a_literal_prefix() {
        let mut tables = KeyTables::default();
        tables.set_prefix("C-Space");

        assert_eq!(tables.prefix(), "C- ");
        assert!(
            tables
                .get("prefix", "C- ")
                .is_some_and(Binding::is_send_prefix)
        );
        let mut engine = KeyEngine::default();
        assert_eq!(engine.handle(&tables, "C- "), KeyDecision::Prefix);
        assert_eq!(
            engine.handle(&tables, "C- "),
            KeyDecision::Commands(vec![CommandInvocation::new("send-prefix", [] as [&str; 0])])
        );
    }

    #[test]
    fn the_prefix_key_sends_a_literal_prefix_by_default() {
        let tables = KeyTables::default();
        let binding = tables.get("prefix", "C-b").expect("send-prefix binding");
        assert!(binding.is_send_prefix());
        assert_eq!(
            binding.commands,
            vec![CommandInvocation::new("send-prefix", [] as [&str; 0])]
        );
    }

    #[test]
    fn changing_the_prefix_carries_the_send_prefix_binding() {
        let mut tables = KeyTables::default();
        tables.set_prefix("C-a");

        assert_eq!(tables.prefix(), "C-a");
        assert!(
            tables
                .get("prefix", "C-a")
                .is_some_and(Binding::is_send_prefix)
        );
        assert!(tables.get("prefix", "C-b").is_none());
    }

    #[test]
    fn changing_the_prefix_never_overwrites_the_new_keys_binding() {
        let mut tables = KeyTables::default();
        let kill = Binding {
            commands: vec![CommandInvocation::new("kill-pane", [] as [&str; 0])],
            repeat: false,
            note: None,
        };
        tables.bind("prefix", "C-a", kill.clone());
        tables.set_prefix("C-a");

        assert_eq!(
            tables.get("prefix", "C-a").map(|b| b.commands.clone()),
            Some(kill.commands)
        );
    }

    #[test]
    fn changing_the_prefix_leaves_a_customized_binding_alone() {
        let mut tables = KeyTables::default();
        tables.bind(
            "prefix",
            "C-b",
            Binding {
                commands: vec![CommandInvocation::new("new-window", [] as [&str; 0])],
                repeat: false,
                note: None,
            },
        );
        tables.set_prefix("C-a");

        assert_eq!(
            tables.get("prefix", "C-b").map(|b| b.commands.clone()),
            Some(vec![CommandInvocation::new("new-window", [] as [&str; 0])])
        );
        assert!(tables.get("prefix", "C-a").is_none());
    }

    #[test]
    fn a_second_prefix_arms_and_rearms_without_touching_bindings() {
        let mut tables = KeyTables::default();
        assert_eq!(tables.prefix2(), None);
        assert_eq!(
            KeyEngine::default().handle(&tables, "C-a"),
            KeyDecision::Pass
        );

        tables.set_prefix2(Some("Ctrl-a"));
        assert_eq!(tables.prefix2(), Some("C-a"));
        assert!(tables.get("prefix", "C-a").is_none());
        assert!(
            tables
                .get("prefix", "C-b")
                .is_some_and(Binding::is_send_prefix)
        );

        let mut engine = KeyEngine::default();
        assert_eq!(engine.handle(&tables, "C-a"), KeyDecision::Prefix);
        assert!(matches!(
            engine.handle(&tables, "c"),
            KeyDecision::Commands(_)
        ));
        assert_eq!(engine.handle(&tables, "C-b"), KeyDecision::Prefix);
        assert_eq!(engine.handle(&tables, "C-a"), KeyDecision::Ignore);

        tables.set_prefix2(None);
        assert_eq!(tables.prefix2(), None);
        let mut engine = KeyEngine::default();
        assert_eq!(engine.handle(&tables, "C-a"), KeyDecision::Pass);
    }

    #[test]
    fn a_none_valued_second_prefix_reads_as_unset() {
        let mut tables = KeyTables::default();
        tables.set_prefix2(Some("None"));
        assert_eq!(tables.prefix2(), None);
        tables.set_prefix2(Some("none"));
        assert_eq!(tables.prefix2(), None);
    }

    #[test]
    fn prefix_table_executes_then_resets() {
        let tables = KeyTables::default();
        let mut engine = KeyEngine::default();
        assert_eq!(engine.handle(&tables, "C-b"), KeyDecision::Prefix);
        assert!(matches!(
            engine.handle(&tables, "c"),
            KeyDecision::Commands(_)
        ));
        assert_eq!(engine.handle(&tables, "c"), KeyDecision::Pass);
        assert_eq!(
            tables.get("prefix", ":").unwrap().commands,
            vec![CommandInvocation::new("command-prompt", [] as [&str; 0])]
        );
        assert_eq!(
            tables.get("prefix", "$").unwrap().commands,
            vec![CommandInvocation::new(
                "command-prompt",
                ["-I", "#S", "rename-session -- '%%'"],
            )]
        );
        assert_eq!(
            tables.get("prefix", ",").unwrap().commands,
            vec![CommandInvocation::new(
                "command-prompt",
                ["-I", "#W", "rename-window -- '%%'"],
            )]
        );
        assert_eq!(
            tables.get("prefix", "%").unwrap().commands,
            vec![CommandInvocation::new("split-picker", ["-h"])]
        );
        assert_eq!(
            tables.get("prefix", "\"").unwrap().commands,
            vec![CommandInvocation::new("split-picker", ["-v"])]
        );
        assert_eq!(
            tables.get("prefix", "?").unwrap().commands,
            vec![CommandInvocation::new("list-keys", [] as [&str; 0])]
        );
        assert_eq!(
            tables.get("prefix", "=").unwrap().commands,
            vec![CommandInvocation::new("choose-buffer", ["-Z"])]
        );
        assert_eq!(
            tables.get("prefix", "s").unwrap().commands,
            vec![CommandInvocation::new("focus-sidebar", [] as [&str; 0])]
        );
        assert_eq!(
            tables.get("prefix", "w").unwrap().commands,
            vec![CommandInvocation::new("focus-sidebar", [] as [&str; 0])]
        );
        assert_eq!(
            tables.get("prefix", "q").unwrap().commands,
            vec![CommandInvocation::new("display-panes", [] as [&str; 0])]
        );
        assert_eq!(
            tables.get("prefix", "r").unwrap().commands,
            vec![CommandInvocation::new("reload-config", [] as [&str; 0])]
        );
        assert_eq!(
            tables.get("prefix", "z").unwrap().commands,
            vec![CommandInvocation::new("resize-pane", ["-Z"])]
        );
        assert_eq!(
            tables.get("prefix", ";").unwrap().commands,
            vec![CommandInvocation::new("last-pane", [] as [&str; 0])]
        );
        assert_eq!(
            tables.get("prefix", "!").unwrap().commands,
            vec![CommandInvocation::new("break-pane", [] as [&str; 0])]
        );
        assert_eq!(
            tables.get("prefix", "Space").unwrap().commands,
            vec![CommandInvocation::new("next-layout", [] as [&str; 0])]
        );
        assert_eq!(
            tables.get("prefix", "C-o").unwrap().commands,
            vec![CommandInvocation::new("rotate-window", [] as [&str; 0],)]
        );
        assert_eq!(
            tables.get("prefix", "M-o").unwrap().commands,
            vec![CommandInvocation::new("rotate-window", ["-D"])]
        );
        assert_eq!(
            tables.get("prefix", "o").unwrap().commands,
            vec![CommandInvocation::new("select-pane", ["-t:.+"])]
        );
        assert_eq!(
            tables.get("prefix", "E").unwrap().commands,
            vec![CommandInvocation::new("select-layout", ["-E"])]
        );
        assert_eq!(
            tables.get("prefix", "M-1").unwrap().commands,
            vec![CommandInvocation::new("select-layout", ["even-horizontal"],)]
        );
        assert_eq!(
            tables.get("prefix", "M-7").unwrap().commands,
            vec![CommandInvocation::new(
                "select-layout",
                ["main-vertical-mirrored"],
            )]
        );
        assert_eq!(
            tables.get("prefix", "{").unwrap().commands,
            vec![CommandInvocation::new("swap-pane", ["-U"])]
        );
        assert_eq!(
            tables.get("prefix", "}").unwrap().commands,
            vec![CommandInvocation::new("swap-pane", ["-D"])]
        );
        let left = tables.get("prefix", "Left").unwrap();
        assert!(left.repeat);
        assert_eq!(
            left.commands,
            vec![CommandInvocation::new("select-pane", ["-L"])]
        );
        assert_eq!(
            tables.get("prefix", "M-Right").unwrap().commands,
            vec![CommandInvocation::new("resize-pane", ["-R", "5"])]
        );
        assert_eq!(
            tables.get("prefix", "C-Up").unwrap().commands,
            vec![CommandInvocation::new("resize-pane", ["-U", "1"])]
        );

        assert_eq!(engine.handle(&tables, "C-b"), KeyDecision::Prefix);
        assert!(matches!(
            engine.handle(&tables, "Left"),
            KeyDecision::Commands(_)
        ));
        assert_eq!(engine.active_table(), Some("prefix"));
        assert!(matches!(
            engine.handle(&tables, "Right"),
            KeyDecision::Commands(_)
        ));
        assert_eq!(engine.active_table(), Some("prefix"));
    }

    #[test]
    fn repeatable_bindings_refresh_expire_disable_and_retry_root() {
        let mut tables = KeyTables::default();
        tables.bind(
            "root",
            "c",
            Binding {
                commands: vec![CommandInvocation::new("display-message", ["root"])],
                repeat: false,
                note: None,
            },
        );
        let start = Instant::now();
        let repeat_time = Duration::from_millis(100);
        let mut engine = KeyEngine::default();

        assert_eq!(
            engine.handle_with_repeat_time(&tables, "C-b", start, repeat_time),
            KeyDecision::Prefix
        );
        assert!(matches!(
            engine.handle_with_repeat_time(&tables, "Left", start, repeat_time),
            KeyDecision::Commands(_)
        ));
        assert!(matches!(
            engine.handle_with_repeat_time(
                &tables,
                "Right",
                start + Duration::from_millis(99),
                repeat_time,
            ),
            KeyDecision::Commands(_)
        ));
        assert_eq!(engine.active_table(), Some("prefix"));
        assert_eq!(
            engine.handle_with_repeat_time(
                &tables,
                "Right",
                start + Duration::from_millis(200),
                repeat_time,
            ),
            KeyDecision::Pass
        );
        assert_eq!(engine.active_table(), None);

        assert_eq!(
            engine.handle_with_repeat_time(&tables, "C-b", start, repeat_time),
            KeyDecision::Prefix
        );
        assert!(matches!(
            engine.handle_with_repeat_time(&tables, "Left", start, repeat_time),
            KeyDecision::Commands(_)
        ));
        assert_eq!(
            engine.handle_with_repeat_time(&tables, "c", start, repeat_time),
            KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["root"])])
        );
        assert_eq!(engine.active_table(), None);

        assert_eq!(
            engine.handle_with_repeat_time(&tables, "C-b", start, Duration::ZERO),
            KeyDecision::Prefix
        );
        assert!(matches!(
            engine.handle_with_repeat_time(&tables, "Left", start, Duration::ZERO),
            KeyDecision::Commands(_)
        ));
        assert_eq!(engine.active_table(), None);
        assert_eq!(
            engine.handle_with_repeat_time(&tables, "Right", start, Duration::ZERO),
            KeyDecision::Pass
        );
    }

    #[test]
    fn command_decisions_report_the_binding_repeat_bit() {
        let mut tables = KeyTables::default();
        tables.bind(
            "prefix",
            "R",
            Binding {
                commands: vec![CommandInvocation::new("display-message", ["repeat"])],
                repeat: true,
                note: None,
            },
        );
        let start = Instant::now();
        let mut engine = KeyEngine::default();
        assert_eq!(
            engine.handle_with_repeat_metadata(
                &tables,
                "C-b",
                start,
                Duration::from_millis(500),
                Duration::ZERO,
                Duration::ZERO,
                "root",
            ),
            (KeyDecision::Prefix, false)
        );
        assert_eq!(
            engine.handle_with_repeat_metadata(
                &tables,
                "R",
                start,
                Duration::from_millis(500),
                Duration::ZERO,
                Duration::ZERO,
                "root",
            ),
            (
                KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["repeat"],)]),
                true,
            )
        );
        engine.switch_table(None);
        assert_eq!(
            engine.handle_with_repeat_metadata(
                &tables,
                "C-b",
                start,
                Duration::from_millis(500),
                Duration::ZERO,
                Duration::ZERO,
                "root",
            ),
            (KeyDecision::Prefix, false)
        );
        assert!(matches!(
            engine.handle_with_repeat_metadata(
                &tables,
                "c",
                start,
                Duration::from_millis(500),
                Duration::ZERO,
                Duration::ZERO,
                "root",
            ),
            (KeyDecision::Commands(_), false)
        ));
    }

    #[test]
    fn initial_repeat_time_applies_to_first_and_different_repeat_bindings() {
        let tables = KeyTables::default();
        let start = Instant::now();
        let repeat_time = Duration::from_millis(100);
        let initial_repeat_time = Duration::from_millis(300);
        let mut engine = KeyEngine::default();

        assert_eq!(
            engine.handle_with_repeat_times(
                &tables,
                "C-b",
                start,
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Prefix
        );
        assert!(matches!(
            engine.handle_with_repeat_times(
                &tables,
                "Left",
                start,
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Commands(_)
        ));
        assert!(matches!(
            engine.handle_with_repeat_times(
                &tables,
                "Left",
                start + Duration::from_millis(299),
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Commands(_)
        ));
        assert_eq!(
            engine.handle_with_repeat_times(
                &tables,
                "Left",
                start + Duration::from_millis(399),
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Pass
        );

        let start = start + Duration::from_secs(1);
        assert_eq!(
            engine.handle_with_repeat_times(
                &tables,
                "C-b",
                start,
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Prefix
        );
        assert!(matches!(
            engine.handle_with_repeat_times(
                &tables,
                "Left",
                start,
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Commands(_)
        ));
        assert!(matches!(
            engine.handle_with_repeat_times(
                &tables,
                "Right",
                start + Duration::from_millis(50),
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Commands(_)
        ));
        assert!(matches!(
            engine.handle_with_repeat_times(
                &tables,
                "Right",
                start + Duration::from_millis(349),
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Commands(_)
        ));
        assert_eq!(
            engine.handle_with_repeat_times(
                &tables,
                "Right",
                start + Duration::from_millis(449),
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Pass
        );

        let start = start + Duration::from_secs(1);
        assert_eq!(
            engine.handle_with_repeat_times(
                &tables,
                "C-b",
                start,
                Duration::ZERO,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Prefix
        );
        assert!(matches!(
            engine.handle_with_repeat_times(
                &tables,
                "Left",
                start,
                Duration::ZERO,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Commands(_)
        ));
        assert_eq!(
            engine.handle_with_repeat_times(
                &tables,
                "Left",
                start + Duration::from_millis(1),
                Duration::ZERO,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Pass
        );
    }

    #[test]
    fn repeat_any_uses_the_resolved_binding_identity() {
        let mut tables = KeyTables::default();
        tables.bind(
            "prefix",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new("display-message", ["any"])],
                repeat: true,
                note: None,
            },
        );
        let start = Instant::now();
        let repeat_time = Duration::from_millis(100);
        let initial_repeat_time = Duration::from_millis(300);
        let mut engine = KeyEngine::default();

        assert_eq!(
            engine.handle_with_repeat_times(
                &tables,
                "C-b",
                start,
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Prefix
        );
        assert!(matches!(
            engine.handle_with_repeat_times(
                &tables,
                "F13",
                start,
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Commands(_)
        ));
        assert!(matches!(
            engine.handle_with_repeat_times(
                &tables,
                "F14",
                start + Duration::from_millis(150),
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Commands(_)
        ));
        assert_eq!(
            engine.handle_with_repeat_times(
                &tables,
                "F15",
                start + Duration::from_millis(250),
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Pass
        );
    }

    #[test]
    fn synthetic_focus_dispatches_only_any_in_effective_default_tables() {
        let mut tables = KeyTables::empty();
        for table in ["root", "custom"] {
            for name in ["FocusIn", "FocusOut"] {
                tables.bind(
                    table,
                    name,
                    Binding {
                        commands: vec![CommandInvocation::new(
                            "display-message",
                            [format!("exact-{name}")],
                        )],
                        repeat: false,
                        note: None,
                    },
                );
            }
            tables.bind(
                table,
                "Any",
                Binding {
                    commands: vec![CommandInvocation::new("display-message", [table])],
                    repeat: false,
                    note: None,
                },
            );
        }
        assert!(!is_key_name("FocusIn"));
        assert!(!is_key_name("FocusOut"));

        for table in ["root", "custom"] {
            let mut engine = KeyEngine::default();
            assert_eq!(
                engine.handle_synthetic_any_with_repeat_metadata(
                    &tables,
                    Instant::now(),
                    Duration::from_millis(500),
                    Duration::ZERO,
                    table,
                ),
                (
                    KeyDecision::Commands(vec![
                        CommandInvocation::new("display-message", [table],)
                    ]),
                    false,
                ),
                "table={table}",
            );
        }

        let mut explicit_custom = KeyEngine::default();
        explicit_custom.switch_table(Some("custom".to_owned()));
        assert_eq!(
            explicit_custom.handle_synthetic_any_with_repeat_metadata(
                &tables,
                Instant::now(),
                Duration::from_millis(500),
                Duration::ZERO,
                "custom",
            ),
            (
                KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["custom"],)]),
                false,
            )
        );
        assert_eq!(explicit_custom.active_table(), Some("custom"));
    }

    #[test]
    fn synthetic_any_explicit_custom_table_falls_back_and_retires() {
        let mut tables = KeyTables::empty();
        for (table, label) in [("root", "root"), ("switched", "switched")] {
            tables.bind(
                table,
                "Any",
                Binding {
                    commands: vec![CommandInvocation::new("display-message", [label])],
                    repeat: false,
                    note: None,
                },
            );
        }

        let mut matched = KeyEngine::default();
        matched.switch_table(Some("switched".to_owned()));
        assert_eq!(
            matched.handle_synthetic_any_with_repeat_metadata(
                &tables,
                Instant::now(),
                Duration::from_millis(500),
                Duration::ZERO,
                "root",
            ),
            (
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "display-message",
                    ["switched"],
                )]),
                false,
            )
        );
        assert_eq!(matched.active_table(), None);

        tables.bind(
            "switched",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new(
                    "display-message",
                    ["switched-repeat"],
                )],
                repeat: true,
                note: None,
            },
        );
        let start = Instant::now();
        let mut repeating = KeyEngine::default();
        repeating.switch_table(Some("switched".to_owned()));
        for now in [start, start + Duration::from_millis(150)] {
            assert_eq!(
                repeating.handle_synthetic_any_with_repeat_metadata(
                    &tables,
                    now,
                    Duration::from_millis(100),
                    Duration::from_millis(300),
                    "root",
                ),
                (
                    KeyDecision::Commands(vec![CommandInvocation::new(
                        "display-message",
                        ["switched-repeat"],
                    )]),
                    true,
                )
            );
            assert_eq!(repeating.active_table(), Some("switched"));
        }
        assert_eq!(
            repeating.handle_synthetic_any_with_repeat_metadata(
                &tables,
                start + Duration::from_millis(250),
                Duration::from_millis(100),
                Duration::from_millis(300),
                "root",
            ),
            (
                KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["root"],)]),
                false,
            )
        );
        assert_eq!(repeating.active_table(), None);

        tables.unbind("switched", "Any");
        let mut fallback = KeyEngine::default();
        fallback.switch_table(Some("switched".to_owned()));
        assert_eq!(
            fallback.handle_synthetic_any_with_repeat_metadata(
                &tables,
                Instant::now(),
                Duration::from_millis(500),
                Duration::ZERO,
                "root",
            ),
            (
                KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["root"],)]),
                false,
            )
        );
        assert_eq!(fallback.active_table(), None);
    }

    #[test]
    fn transient_mode_synthetic_any_precedes_default_without_leaving_mode() {
        let mut tables = KeyTables::empty();
        for (table, label) in [("copy-mode-vi", "mode"), ("custom", "default")] {
            tables.bind(
                table,
                "Any",
                Binding {
                    commands: vec![CommandInvocation::new("display-message", [label])],
                    repeat: false,
                    note: None,
                },
            );
        }
        tables.bind(
            "copy-mode-vi",
            "FocusIn",
            Binding {
                commands: vec![CommandInvocation::new("display-message", ["exact-focus"])],
                repeat: false,
                note: None,
            },
        );
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));

        assert_eq!(
            engine.handle_transient_mode_synthetic_any(&tables, "copy-mode-vi", "custom"),
            (
                KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["mode"],)]),
                false,
            )
        );
        assert_eq!(engine.active_table(), Some("copy-mode-vi"));

        tables.unbind("copy-mode-vi", "Any");
        assert_eq!(
            engine.handle_transient_mode_synthetic_any(&tables, "copy-mode-vi", "custom"),
            (
                KeyDecision::Commands(vec![
                    CommandInvocation::new("display-message", ["default"],)
                ]),
                false,
            )
        );
        assert_eq!(engine.active_table(), Some("copy-mode-vi"));
    }

    #[test]
    fn transient_synthetic_jump_any_preserves_pending_jump_target() {
        let mut tables = KeyTables::default();
        tables.bind(
            "copy-mode-vi",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new("send-keys", ["-X", "jump-backward"])],
                repeat: false,
                note: None,
            },
        );
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));

        assert_eq!(engine.handle(&tables, "f"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle_transient_mode_synthetic_any(&tables, "copy-mode-vi", "root"),
            (KeyDecision::Ignore, false)
        );
        assert_eq!(
            engine.handle(&tables, "x"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-X", "jump-forward", "x"],
            )])
        );
    }

    #[test]
    fn transient_synthetic_jump_any_does_not_claim_the_next_real_key() {
        let mut tables = KeyTables::default();
        tables.bind(
            "copy-mode-vi",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new("send-keys", ["-X", "jump-backward"])],
                repeat: false,
                note: None,
            },
        );
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));

        assert_eq!(
            engine.handle_transient_mode_synthetic_any(&tables, "copy-mode-vi", "root"),
            (KeyDecision::Ignore, false)
        );
        assert_eq!(
            engine.handle(&tables, "h"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-X", "cursor-left"],
            )])
        );
    }

    #[test]
    fn synthetic_any_honors_prefix_repeat_nonrepeat_and_expiry() {
        let mut tables = KeyTables::empty();
        tables.bind(
            "root",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new("display-message", ["root"])],
                repeat: false,
                note: None,
            },
        );
        tables.bind(
            "prefix",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new("display-message", ["prefix-repeat"])],
                repeat: true,
                note: None,
            },
        );
        let start = Instant::now();
        let repeat_time = Duration::from_millis(100);
        let initial_repeat_time = Duration::from_millis(300);
        let prefix_timeout = Duration::from_millis(100);
        let mut repeat = KeyEngine::default();
        assert_eq!(
            repeat.handle_with_repeat_times(
                &tables,
                "C-b",
                start,
                repeat_time,
                initial_repeat_time,
                Duration::ZERO,
                "root",
            ),
            KeyDecision::Prefix
        );
        for now in [start, start + Duration::from_millis(150)] {
            assert_eq!(
                repeat.handle_synthetic_any_with_repeat_metadata(
                    &tables,
                    now,
                    repeat_time,
                    initial_repeat_time,
                    "root",
                ),
                (
                    KeyDecision::Commands(vec![CommandInvocation::new(
                        "display-message",
                        ["prefix-repeat"],
                    )]),
                    true,
                )
            );
            assert_eq!(repeat.active_table(), Some("prefix"));
        }
        assert_eq!(
            repeat.handle_synthetic_any_with_repeat_metadata(
                &tables,
                start + Duration::from_millis(250),
                repeat_time,
                initial_repeat_time,
                "root",
            ),
            (
                KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["root"],)]),
                false,
            )
        );
        assert_eq!(repeat.active_table(), None);

        tables.bind(
            "prefix",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new("display-message", ["prefix-once"])],
                repeat: false,
                note: None,
            },
        );
        let mut nonrepeat = KeyEngine::default();
        nonrepeat.handle_with_repeat_times(
            &tables,
            "C-b",
            start,
            repeat_time,
            initial_repeat_time,
            prefix_timeout,
            "root",
        );
        assert_eq!(
            nonrepeat.handle_synthetic_any_with_repeat_metadata(
                &tables,
                start,
                repeat_time,
                initial_repeat_time,
                "root",
            ),
            (
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "display-message",
                    ["prefix-once"],
                )]),
                false,
            )
        );
        assert_eq!(nonrepeat.active_table(), None);

        tables.bind(
            "prefix",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new(
                    "display-message",
                    ["expired-prefix"],
                )],
                repeat: true,
                note: None,
            },
        );
        let mut expired = KeyEngine::default();
        expired.handle_with_repeat_times(
            &tables,
            "C-b",
            start,
            repeat_time,
            initial_repeat_time,
            prefix_timeout,
            "root",
        );
        assert_eq!(
            expired.handle_synthetic_any_with_repeat_metadata(
                &tables,
                start,
                repeat_time,
                initial_repeat_time,
                "root",
            ),
            (
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "display-message",
                    ["expired-prefix"],
                )]),
                true,
            )
        );
        assert_eq!(expired.active_table(), Some("prefix"));
        assert_eq!(expired.repeat_deadline, Some(start + initial_repeat_time));
        assert_eq!(
            expired.handle_synthetic_any_with_repeat_metadata(
                &tables,
                start + prefix_timeout + Duration::from_millis(1),
                repeat_time,
                initial_repeat_time,
                "root",
            ),
            (
                KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["root"],)]),
                false,
            )
        );
        assert_eq!(expired.active_table(), None);
    }

    #[test]
    fn root_and_custom_bindings_work() {
        let mut tables = KeyTables::default();
        tables.bind(
            "root",
            "F2",
            Binding {
                commands: vec![CommandInvocation::new("new-window", [] as [&str; 0])],
                repeat: false,
                note: None,
            },
        );
        let mut engine = KeyEngine::default();
        assert!(matches!(
            engine.handle(&tables, "F2"),
            KeyDecision::Commands(_)
        ));
        tables.bind(
            "custom",
            "F3",
            Binding {
                commands: vec![CommandInvocation::new("display-message", ["custom"])],
                repeat: false,
                note: None,
            },
        );
        assert_eq!(
            engine.handle_with_repeat_times(
                &tables,
                "F3",
                Instant::now(),
                Duration::ZERO,
                Duration::ZERO,
                Duration::ZERO,
                "custom",
            ),
            KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["custom"],)])
        );
    }

    #[test]
    fn prefix_timeout_expires_lazily_and_exempts_exact_active_repeat_bindings() {
        let mut tables = KeyTables::default();
        for (table, key, label, repeat) in [
            ("root", "x", "root", false),
            ("prefix", "x", "prefix", false),
            ("root", "r", "root-repeat", false),
            ("prefix", "r", "prefix-repeat", true),
            ("root", "F13", "root-any", false),
            ("prefix", "Any", "prefix-any", true),
        ] {
            tables.bind(
                table,
                key,
                Binding {
                    commands: vec![CommandInvocation::new("display-message", [label])],
                    repeat,
                    note: None,
                },
            );
        }
        let start = Instant::now();
        let timeout = Duration::from_millis(100);
        let repeat_time = Duration::from_millis(500);

        let mut boundary = KeyEngine::default();
        assert_eq!(
            boundary.handle_with_repeat_times(
                &tables,
                "C-b",
                start,
                repeat_time,
                Duration::ZERO,
                timeout,
                "root",
            ),
            KeyDecision::Prefix
        );
        assert_eq!(
            boundary.handle_with_repeat_times(
                &tables,
                "x",
                start + timeout,
                repeat_time,
                Duration::ZERO,
                timeout,
                "root",
            ),
            KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["prefix"],)])
        );

        let mut expired = KeyEngine::default();
        expired.handle_with_repeat_times(
            &tables,
            "C-b",
            start,
            repeat_time,
            Duration::ZERO,
            timeout,
            "root",
        );
        assert_eq!(
            expired.handle_with_repeat_times(
                &tables,
                "x",
                start + timeout + Duration::from_millis(1),
                repeat_time,
                Duration::ZERO,
                timeout,
                "root",
            ),
            KeyDecision::Commands(vec![CommandInvocation::new("display-message", ["root"],)])
        );

        let mut repeat = KeyEngine::default();
        repeat.handle_with_repeat_times(
            &tables,
            "C-b",
            start,
            repeat_time,
            Duration::ZERO,
            timeout,
            "root",
        );
        repeat.handle_with_repeat_times(
            &tables,
            "r",
            start + Duration::from_millis(50),
            repeat_time,
            Duration::ZERO,
            timeout,
            "root",
        );
        assert_eq!(
            repeat.handle_with_repeat_times(
                &tables,
                "r",
                start + Duration::from_millis(150),
                repeat_time,
                Duration::ZERO,
                timeout,
                "root",
            ),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "display-message",
                ["prefix-repeat"],
            )])
        );

        let mut any = KeyEngine::default();
        any.handle_with_repeat_times(
            &tables,
            "C-b",
            start,
            repeat_time,
            Duration::ZERO,
            timeout,
            "root",
        );
        assert_eq!(
            any.handle_with_repeat_times(
                &tables,
                "F13",
                start + Duration::from_millis(101),
                repeat_time,
                Duration::ZERO,
                timeout,
                "root",
            ),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "display-message",
                ["root-any"],
            )])
        );
    }

    #[test]
    fn native_copy_mode_search_bindings_match_tmux_tables() {
        let tables = KeyTables::default();
        assert_eq!(
            tables.get("copy-mode-vi", "/").unwrap().commands,
            vec![CommandInvocation::new(
                "copy-mode-search-prompt",
                [] as [&str; 0],
            )]
        );
        assert_eq!(
            tables.get("copy-mode-vi", "?").unwrap().commands,
            vec![CommandInvocation::new("copy-mode-search-prompt", ["-b"])]
        );
        assert_eq!(
            tables.get("copy-mode-vi", "n").unwrap().commands,
            vec![CommandInvocation::new("send-keys", ["-X", "search-again"],)]
        );
        assert_eq!(
            tables.get("copy-mode-vi", "N").unwrap().commands,
            vec![CommandInvocation::new(
                "send-keys",
                ["-X", "search-reverse"],
            )]
        );
    }

    #[test]
    fn stock_copy_tables_keep_runtime_repetition_out_of_binding_metadata() {
        let mut tables = KeyTables::default();
        for table in ["copy-mode", "copy-mode-vi"] {
            let bindings = tables.tables.get(table).expect("stock copy table");
            assert!(!bindings.is_empty());
            assert!(bindings.values().all(|binding| !binding.repeat));
        }
        assert!(tables.get("prefix", "Left").expect("prefix repeat").repeat);
        assert!(
            tables
                .get("choose-tree", "Up")
                .expect("chooser repeat")
                .repeat
        );
        tables.bind(
            "user",
            "x",
            Binding {
                commands: vec![CommandInvocation::new("display-message", ["user"])],
                repeat: true,
                note: None,
            },
        );
        assert!(tables.get("user", "x").expect("user repeat").repeat);

        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));
        assert_eq!(
            engine.handle(&tables, "h"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-X", "cursor-left"],
            )])
        );
        assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
        assert_eq!(engine.handle(&tables, "f"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "x"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "3", "-X", "jump-forward", "x"],
            )])
        );
    }

    #[test]
    fn copy_table_exit_keys_match_tmux_semantics() {
        let tables = KeyTables::default();
        let action = |table, key| {
            tables
                .get(table, key)
                .unwrap_or_else(|| panic!("{table} {key} is bound"))
                .commands
                .clone()
        };
        let sends = |name: &str| vec![CommandInvocation::new("send-keys", ["-X", name])];

        assert_eq!(action("copy-mode-vi", "Escape"), sends("clear-selection"));
        for key in ["q", "C-c"] {
            assert_eq!(action("copy-mode-vi", key), sends("cancel"));
        }
        for key in ["Escape", "q", "C-c"] {
            assert_eq!(action("copy-mode", key), sends("cancel"));
        }
    }

    #[test]
    fn copy_mode_emacs_m_f_moves_to_the_next_word_end() {
        let tables = KeyTables::default();
        assert_eq!(
            tables
                .get("copy-mode", "M-f")
                .expect("M-f binding")
                .commands,
            vec![CommandInvocation::new("send-keys", ["-X", "next-word-end"],)]
        );
    }

    #[test]
    fn stock_copy_mode_emacs_navigation_bindings_match_the_pin() {
        let tables = KeyTables::default();
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode".to_owned()));

        for (key, action) in [
            ("C-Down", "scroll-down"),
            ("C-M-Down", "next-prompt"),
            ("C-M-Up", "previous-prompt"),
            ("C-M-f", "next-matching-bracket"),
            ("C-Up", "scroll-up"),
            ("End", "end-of-line"),
            ("Home", "start-of-line"),
            ("M-<", "history-top"),
            ("M->", "history-bottom"),
            ("M-Down", "halfpage-down"),
            ("M-R", "top-line"),
            ("M-Up", "halfpage-up"),
            ("Space", "page-down"),
        ] {
            let expected = vec![CommandInvocation::new("send-keys", ["-X", action])];
            let binding = tables
                .get("copy-mode", key)
                .unwrap_or_else(|| panic!("copy-mode {key} is bound"));
            assert_eq!(binding.commands, expected, "copy-mode {key}");
            assert!(!binding.repeat, "copy-mode {key}");
            assert_eq!(
                engine.handle(&tables, key),
                KeyDecision::Commands(expected),
                "copy-mode {key}"
            );
        }
    }

    #[test]
    fn stock_copy_mode_emacs_non_navigation_bindings_match_the_pin() {
        let tables = KeyTables::default();
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode".to_owned()));

        for (key, action) in [
            ("C-[", "cancel"),
            ("C-k", "copy-pipe-end-of-line-and-cancel"),
            ("C-w", "copy-pipe-and-cancel"),
            ("N", "search-reverse"),
            ("R", "rectangle-toggle"),
            ("n", "search-again"),
        ] {
            let expected = vec![CommandInvocation::new("send-keys", ["-X", action])];
            let binding = tables
                .get("copy-mode", key)
                .unwrap_or_else(|| panic!("copy-mode {key} is bound"));
            assert_eq!(binding.commands, expected, "copy-mode {key}");
            assert!(!binding.repeat, "copy-mode {key}");
            assert_eq!(
                engine.handle(&tables, key),
                KeyDecision::Commands(expected),
                "copy-mode {key}"
            );
        }

        for (key, action) in [
            ("Escape", "cancel"),
            ("M-w", "copy-pipe-and-cancel"),
            ("C-g", "clear-selection"),
        ] {
            assert_eq!(
                tables
                    .get("copy-mode", key)
                    .unwrap_or_else(|| panic!("copy-mode {key} remains bound"))
                    .commands,
                vec![CommandInvocation::new("send-keys", ["-X", action])],
                "copy-mode {key}"
            );
        }
    }

    #[test]
    fn native_copy_mode_emacs_keyboard_table_matches_the_audited_key_set() {
        let tables = KeyTables::default();
        let expected = [
            " ", ",", ";", "C- ", "C-Down", "C-M-Down", "C-M-Up", "C-M-f", "C-Up", "C-[", "C-a",
            "C-b", "C-c", "C-e", "C-f", "C-g", "C-k", "C-n", "C-p", "C-r", "C-s", "C-v", "C-w",
            "Down", "End", "Enter", "Escape", "F", "Home", "Left", "M-<", "M->", "M-Down", "M-R",
            "M-Up", "M-b", "M-f", "M-m", "M-r", "M-v", "M-w", "M-x", "M-{", "M-}", "N", "NPage",
            "PPage", "R", "Right", "T", "Up", "X", "f", "n", "q", "t",
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
        let actual = tables.tables["copy-mode"]
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(actual, expected);
    }

    #[test]
    fn shifted_alt_emacs_navigation_names_reach_the_stock_bindings() {
        let tables = KeyTables::default();
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode".to_owned()));
        let alt_shift = Modifiers::new(true, false, true, false);

        for (character, key, action) in [
            ('R', "M-R", "top-line"),
            ('<', "M-<", "history-top"),
            ('>', "M->", "history-bottom"),
        ] {
            let input = press(
                KeyCode::Character(character),
                alt_shift,
                Some(&character.to_string()),
            );
            assert_eq!(input_key_name(&input).as_str(), key);
            assert_eq!(
                engine.handle(&tables, key),
                KeyDecision::Commands(vec![CommandInvocation::new("send-keys", ["-X", action])])
            );
        }
    }

    #[test]
    fn native_copy_mode_vi_bindings_cover_pinned_tmux_motions_and_aliases() {
        let tables = KeyTables::default();
        let sends = |name: &str| vec![CommandInvocation::new("send-keys", ["-X", name])];
        for (key, action) in [
            ("C-h", "cursor-left"),
            ("BSpace", "cursor-left"),
            ("C-v", "rectangle-toggle"),
            ("C-[", "clear-selection"),
            ("Home", "start-of-line"),
            ("End", "end-of-line"),
            ("B", "previous-space"),
            ("E", "next-space-end"),
            ("W", "next-space"),
            ("C-y", "scroll-up"),
            ("K", "scroll-up"),
            ("C-Up", "scroll-up"),
            ("C-e", "scroll-down"),
            ("J", "scroll-down"),
            ("C-Down", "scroll-down"),
            ("z", "scroll-middle"),
            ("%", "next-matching-bracket"),
            ("D", "copy-pipe-end-of-line-and-cancel"),
            ("#", "search-backward-cursor-word"),
            ("*", "search-forward-cursor-word"),
        ] {
            assert_eq!(
                tables
                    .get("copy-mode-vi", key)
                    .unwrap_or_else(|| panic!("copy-mode-vi {key} is bound"))
                    .commands,
                sends(action),
                "copy-mode-vi {key}",
            );
        }
        assert_eq!(
            tables.get("copy-mode-vi", ":").expect(": binding").commands,
            vec![CommandInvocation::new(
                "command-prompt",
                ["-p", "(goto line)", "send-keys -X goto-line -- '%%'",],
            )]
        );
        for digit in '1'..='9' {
            assert!(
                tables.get("copy-mode-vi", &digit.to_string()).is_some(),
                "copy-mode-vi {digit}"
            );
        }
    }

    #[test]
    fn native_copy_mode_vi_keyboard_table_matches_the_audited_key_set() {
        let tables = KeyTables::default();
        let expected = [
            "#", "*", "C-c", "C-d", "C-e", "C-b", "C-f", "C-h", "C-j", "Enter", "C-u", "C-v",
            "C-y", "Escape", "C-[", " ", "$", ",", "/", "0", "1", "2", "3", "4", "5", "6", "7",
            "8", "9", ":", ";", "?", "A", "B", "D", "E", "F", "G", "H", "J", "K", "L", "M", "N",
            "T", "V", "W", "X", "^", "b", "e", "f", "g", "h", "j", "k", "z", "l", "n", "o", "q",
            "t", "v", "w", "{", "}", "%", "Home", "End", "BSpace", "NPage", "PPage", "Up", "Down",
            "Left", "Right", "M-x", "C-Up", "C-Down",
        ]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
        let actual = tables.tables["copy-mode-vi"]
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(actual, expected);
        assert_eq!(actual.len(), 79);
    }

    #[test]
    fn configured_v_motion_y_sequences_remain_composable() {
        let mut tables = KeyTables::default();
        for (key, action) in [("v", "begin-selection"), ("y", "copy-selection-and-cancel")] {
            tables.bind(
                "copy-mode-vi",
                key,
                Binding {
                    commands: vec![CommandInvocation::new("send-keys", ["-X", action])],
                    repeat: false,
                    note: None,
                },
            );
        }
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));
        for (key, action) in [
            ("v", "begin-selection"),
            ("E", "next-space-end"),
            ("y", "copy-selection-and-cancel"),
        ] {
            assert_eq!(
                engine.handle(&tables, key),
                KeyDecision::Commands(vec![CommandInvocation::new("send-keys", ["-X", action]),])
            );
        }
        assert_eq!(engine.active_table(), Some("copy-mode-vi"));
    }

    #[test]
    fn copy_mode_vi_numeric_prefix_carries_count_into_one_action() {
        let tables = KeyTables::default();
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));

        assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "3", "-X", "next-space-end"],
            )])
        );

        assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
        assert_eq!(engine.handle(&tables, "5"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "35", "-X", "next-space-end"],
            )])
        );

        assert_eq!(engine.handle(&tables, "1"), KeyDecision::Ignore);
        assert_eq!(engine.handle(&tables, "0"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "10", "-X", "next-space-end"],
            )])
        );

        assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
        assert_eq!(engine.handle(&tables, "f"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "x"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "3", "-X", "jump-forward", "x"],
            )])
        );

        for (key, action) in [
            ("%", "next-matching-bracket"),
            ("V", "select-line"),
            ("o", "other-end"),
        ] {
            assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
            assert_eq!(
                engine.handle(&tables, key),
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "send-keys",
                    ["-N", "3", "-X", action],
                )]),
                "3{key}",
            );
        }

        for (key, action) in [
            ("v", "rectangle-toggle"),
            ("Space", "begin-selection"),
            ("o", "other-end"),
            ("Enter", "copy-pipe-and-cancel"),
            ("q", "cancel"),
        ] {
            assert_eq!(engine.handle(&tables, "2"), KeyDecision::Ignore);
            assert_eq!(
                engine.handle(&tables, key),
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "send-keys",
                    ["-N", "2", "-X", action],
                )]),
                "2{key}",
            );
        }

        assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "Escape"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "3", "-X", "clear-selection"],
            )])
        );
    }

    #[test]
    fn copy_mode_vi_numeric_prefix_counts_only_the_first_action_in_a_chain() {
        let mut tables = KeyTables::default();
        tables.bind(
            "copy-mode-vi",
            "x",
            Binding {
                commands: vec![
                    CommandInvocation::new("display-message", ["before"]),
                    CommandInvocation::new("send-keys", ["-X", "cursor-right"]),
                    CommandInvocation::new("send-keys", ["-X", "cursor-left"]),
                ],
                repeat: false,
                note: None,
            },
        );
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));

        assert_eq!(engine.handle(&tables, "2"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "x"),
            KeyDecision::Commands(vec![
                CommandInvocation::new("display-message", ["before"]),
                CommandInvocation::new("send-keys", ["-N", "2", "-X", "cursor-right"]),
                CommandInvocation::new("send-keys", ["-X", "cursor-left"]),
            ])
        );
    }

    #[test]
    fn copy_mode_vi_numeric_prefix_waits_for_an_action_and_defers_to_its_repeat() {
        let mut tables = KeyTables::default();
        for (key, command) in [
            (
                "x",
                CommandInvocation::new("send-keys", ["literal-without-copy-action"]),
            ),
            (
                "y",
                CommandInvocation::new("send-keys", ["-N", "7", "-X", "cursor-right"]),
            ),
            ("z", CommandInvocation::new("send", ["-XN2", "cursor-left"])),
        ] {
            tables.bind(
                "copy-mode-vi",
                key,
                Binding {
                    commands: vec![command],
                    repeat: false,
                    note: None,
                },
            );
        }
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));

        assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "x"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["literal-without-copy-action"],
            )])
        );
        assert_eq!(
            engine.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "3", "-X", "next-space-end"],
            )])
        );

        assert_eq!(engine.handle(&tables, "4"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "y"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "7", "-X", "cursor-right"],
            )])
        );
        assert_eq!(
            engine.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-X", "next-space-end"],
            )])
        );

        assert_eq!(engine.handle(&tables, "5"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "z"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send",
                ["-XN2", "cursor-left"],
            )])
        );
        assert_eq!(
            engine.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-X", "next-space-end"],
            )])
        );
    }

    #[test]
    fn externally_armed_copy_prefix_starts_a_fresh_native_digit_capture() {
        let tables = KeyTables::default();
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));
        engine.set_repeat_count(3);

        assert_eq!(engine.handle(&tables, "5"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "5", "-X", "next-space-end"],
            )])
        );
    }

    #[test]
    fn unbound_copy_keys_preserve_armed_and_capturing_prefixes() {
        let tables = KeyTables::default();

        let mut armed = KeyEngine::default();
        armed.switch_table(Some("copy-mode-vi".to_owned()));
        armed.set_repeat_count(3);
        assert_eq!(armed.handle(&tables, "Unbound"), KeyDecision::Ignore);
        assert_eq!(
            armed.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "3", "-X", "next-space-end"],
            )])
        );

        let mut capturing = KeyEngine::default();
        capturing.switch_table(Some("copy-mode-vi".to_owned()));
        assert_eq!(capturing.handle(&tables, "3"), KeyDecision::Ignore);
        assert_eq!(capturing.handle(&tables, "Unbound"), KeyDecision::Ignore);
        assert_eq!(
            capturing.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "3", "-X", "next-space-end"],
            )])
        );
    }

    #[test]
    fn synthetic_any_preserves_and_consumes_both_copy_prefix_states() {
        let mut tables = KeyTables::default();

        for armed in [true, false] {
            let mut engine = KeyEngine::default();
            engine.switch_table(Some("copy-mode-vi".to_owned()));
            if armed {
                engine.set_repeat_count(3);
            } else {
                assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
            }
            assert_eq!(
                engine.handle_transient_mode_synthetic_any(&tables, "copy-mode-vi", "root",),
                (KeyDecision::Ignore, false),
                "armed={armed}",
            );
            assert_eq!(
                engine.handle(&tables, "E"),
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "send-keys",
                    ["-N", "3", "-X", "next-space-end"],
                )]),
                "armed={armed}",
            );
        }

        tables.bind(
            "copy-mode-vi",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new("send-keys", ["-X", "cursor-right"])],
                repeat: false,
                note: None,
            },
        );
        for armed in [true, false] {
            let mut engine = KeyEngine::default();
            engine.switch_table(Some("copy-mode-vi".to_owned()));
            if armed {
                engine.set_repeat_count(4);
            } else {
                assert_eq!(engine.handle(&tables, "4"), KeyDecision::Ignore);
            }
            assert_eq!(
                engine.handle_transient_mode_synthetic_any(&tables, "copy-mode-vi", "root",),
                (
                    KeyDecision::Commands(vec![CommandInvocation::new(
                        "send-keys",
                        ["-N", "4", "-X", "cursor-right"],
                    )]),
                    false,
                ),
                "armed={armed}",
            );
            assert_eq!(
                engine.handle(&tables, "E"),
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "send-keys",
                    ["-X", "next-space-end"],
                )]),
                "armed={armed}",
            );
        }

        tables.bind(
            "copy-mode-vi",
            "Any",
            Binding {
                commands: vec![CommandInvocation::new(
                    "copy-mode-search-prompt",
                    [] as [&str; 0],
                )],
                repeat: false,
                note: None,
            },
        );
        let mut prompt = KeyEngine::default();
        prompt.switch_table(Some("copy-mode-vi".to_owned()));
        assert_eq!(prompt.handle(&tables, "5"), KeyDecision::Ignore);
        assert_eq!(
            prompt.handle_transient_mode_synthetic_any(&tables, "copy-mode-vi", "root"),
            (
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "copy-mode-search-prompt",
                    [] as [&str; 0],
                )]),
                false,
            )
        );
        assert_eq!(
            prompt.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-X", "next-space-end"],
            )])
        );
    }

    #[test]
    fn copy_mode_prompts_consume_native_prefixes_without_leaking_into_later_motion() {
        let tables = KeyTables::default();

        for (key, command) in [
            (
                "/",
                CommandInvocation::new("copy-mode-search-prompt", [] as [&str; 0]),
            ),
            (
                ":",
                CommandInvocation::new(
                    "command-prompt",
                    ["-p", "(goto line)", "send-keys -X goto-line -- '%%'"],
                ),
            ),
        ] {
            let mut engine = KeyEngine::default();
            engine.switch_table(Some("copy-mode-vi".to_owned()));
            assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
            assert_eq!(
                engine.handle(&tables, key),
                KeyDecision::Commands(vec![command])
            );
            assert_eq!(
                engine.handle(&tables, "E"),
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "send-keys",
                    ["-X", "next-space-end"],
                )]),
                "copy-mode {key}",
            );
        }
    }

    #[test]
    fn neutral_external_repeat_counts_clear_live_native_capture() {
        let tables = KeyTables::default();
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));

        for count in [1, 0] {
            assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
            engine.set_repeat_count(count);
            assert_eq!(
                engine.handle(&tables, "E"),
                KeyDecision::Commands(vec![CommandInvocation::new(
                    "send-keys",
                    ["-X", "next-space-end"],
                )]),
                "set_repeat_count({count})",
            );
        }
    }

    #[test]
    fn invalid_copy_action_resets_prefix_before_the_next_digit() {
        let mut tables = KeyTables::default();
        tables.bind(
            "copy-mode-vi",
            "x",
            Binding {
                commands: vec![CommandInvocation::new(
                    "send-keys",
                    ["-X", "unknown-action"],
                )],
                repeat: false,
                note: None,
            },
        );
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));

        assert_eq!(engine.handle(&tables, "3"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "x"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "3", "-X", "unknown-action"],
            )])
        );
        assert_eq!(engine.handle(&tables, "5"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "E"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-N", "5", "-X", "next-space-end"],
            )])
        );
    }

    #[test]
    fn persistent_copy_tables_consume_unbound_keys_without_exiting() {
        let tables = KeyTables::default();
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));
        assert_eq!(engine.handle(&tables, "Unbound"), KeyDecision::Ignore);
        assert!(matches!(
            engine.handle(&tables, "h"),
            KeyDecision::Commands(_)
        ));
    }

    #[test]
    fn copy_jump_bindings_capture_exactly_one_following_key() {
        let tables = KeyTables::default();
        let mut engine = KeyEngine::default();
        engine.switch_table(Some("copy-mode-vi".to_owned()));
        assert_eq!(engine.handle(&tables, "f"), KeyDecision::Ignore);
        assert_eq!(
            engine.handle(&tables, "é"),
            KeyDecision::Commands(vec![CommandInvocation::new(
                "send-keys",
                ["-X", "jump-forward", "é"],
            )])
        );
        assert!(matches!(
            engine.handle(&tables, "h"),
            KeyDecision::Commands(_)
        ));

        assert_eq!(engine.handle(&tables, "t"), KeyDecision::Ignore);
        assert_eq!(engine.handle(&tables, "Escape"), KeyDecision::Ignore);
        assert!(matches!(
            engine.handle(&tables, "l"),
            KeyDecision::Commands(_)
        ));
    }
    #[test]
    fn key_names_accept_only_spellings_a_press_can_produce() {
        for name in [
            "0", "9", "a", "M-a", "M-z", "C-a", "C-M-a", " ", "é", "Enter", "BSpace", "Escape",
            "PPage", "NPage", "Up", "Down", "Left", "Right", "DC", "IC", "Home", "End", "F1",
            "F12", "M-Enter",
        ] {
            assert!(is_key_name(name), "{name} names a pressable key");
        }
        for name in [
            "", "10", "M-", "C-", "None", "nope", "F0", "F13", "M-C-a", "Space", "\u{1}",
        ] {
            assert!(!is_key_name(name), "{name} names no pressable key");
        }
    }

    #[test]
    fn every_pressable_key_name_validates_as_one() {
        let inputs = [
            KeyCode::Character('a'),
            KeyCode::Backspace,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Escape,
            KeyCode::Delete,
            KeyCode::Insert,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::Function(1),
            KeyCode::Function(12),
        ];
        for key in inputs {
            for modifiers in [
                Modifiers::default(),
                Modifiers::new(false, true, false, false),
                Modifiers::new(false, false, true, false),
                Modifiers::new(false, true, true, false),
            ] {
                let name = input_key_name(&press(key, modifiers, None));
                assert!(
                    is_key_name(name.as_str()),
                    "{} came out of input_key_name",
                    name.as_str(),
                );
                assert_eq!(canonical_key(name.as_str()), name.as_str());
            }
        }
    }
}
