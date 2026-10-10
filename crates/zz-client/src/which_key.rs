use zz_protocol::{
    CommandInvocation, KeyBindingSnapshot, KeyTableSnapshot, KeyTables, canonical_command,
    catalog_command_spec,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WhichKeyGroup {
    Panes,
    Windows,
    Sessions,
    CopyPaste,
    Other,
    Yours,
}

impl WhichKeyGroup {
    pub const ORDER: [Self; 6] = [
        Self::Panes,
        Self::Windows,
        Self::Sessions,
        Self::CopyPaste,
        Self::Other,
        Self::Yours,
    ];

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Panes => "Panes",
            Self::Windows => "Windows",
            Self::Sessions => "Sessions",
            Self::CopyPaste => "Copy and paste",
            Self::Other => "Other",
            Self::Yours => "Yours",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhichKeySet {
    pub keys: Vec<String>,
    pub yours: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhichKeyRow {
    pub keys: Vec<WhichKeySet>,
    pub label: String,
    pub group: Option<WhichKeyGroup>,
    pub repeat: bool,
    pub core: bool,
}

impl WhichKeyRow {
    #[must_use]
    pub fn first_key(&self) -> &str {
        self.keys
            .first()
            .and_then(|set| set.keys.first())
            .map_or("", String::as_str)
    }

    #[must_use]
    pub fn yours(&self) -> bool {
        self.keys.iter().any(|set| set.yours)
    }
}

#[must_use]
pub fn opens_all_keys(commands: &[CommandInvocation]) -> bool {
    matches!(commands, [command]
        if canonical_command(&command.name) == "list-keys"
            && command.args.len() == 1
            && &*command.args[0] == "-N")
}

struct Class {
    group: WhichKeyGroup,
    rank: u8,
    core: bool,
    label: String,
    family: Option<Family>,
}

struct Family {
    id: String,
    label: String,
    slot: u8,
}

const DIRECTIONS: [(&str, &str); 4] = [
    ("-L", "left"),
    ("-D", "down"),
    ("-U", "up"),
    ("-R", "right"),
];

const LAYOUTS: [&str; 7] = [
    "even-horizontal",
    "even-vertical",
    "main-horizontal",
    "main-vertical",
    "tiled",
    "main-horizontal-mirrored",
    "main-vertical-mirrored",
];

fn class(group: WhichKeyGroup, rank: u8, core: bool, label: impl Into<String>) -> Class {
    Class {
        group,
        rank,
        core,
        label: label.into(),
        family: None,
    }
}

fn member(
    group: WhichKeyGroup,
    rank: u8,
    core: bool,
    label: impl Into<String>,
    family: (impl Into<String>, impl Into<String>, u8),
) -> Class {
    Class {
        family: Some(Family {
            id: family.0.into(),
            label: family.1.into(),
            slot: family.2,
        }),
        ..class(group, rank, core, label)
    }
}

fn inner(command: &CommandInvocation) -> (String, Vec<String>, bool) {
    let name = canonical_command(&command.name);
    let args: Vec<String> = command.args.iter().map(ToString::to_string).collect();
    if matches!(name, "confirm-before" | "command-prompt")
        && let Some(last) = args.last()
    {
        let mut words = last
            .trim()
            .trim_start_matches('{')
            .trim_end_matches('}')
            .split_whitespace();
        if let Some(inner) = words
            .next()
            .filter(|inner| catalog_command_spec(inner).is_some())
        {
            return (
                canonical_command(inner).to_owned(),
                words.map(str::to_owned).collect(),
                name == "command-prompt",
            );
        }
    }
    (name.to_owned(), args, false)
}

fn classify(commands: &[CommandInvocation]) -> Option<Class> {
    use WhichKeyGroup::{CopyPaste, Other, Panes, Sessions, Windows};
    let [command, rest @ ..] = commands else {
        return None;
    };
    if let [.., last] = rest
        && canonical_command(&command.name) == "new-pane"
        && canonical_command(&last.name) == "switch-mode"
    {
        let windows = last.args.iter().any(|arg| {
            let arg = arg.to_string();
            arg.starts_with('-') && arg.contains('w')
        });
        return Some(if windows {
            class(Windows, 11, false, "Switch to a window")
        } else {
            class(Sessions, 9, false, "Switch to a session")
        });
    }
    if !rest.iter().all(is_feedback) {
        return None;
    }
    let (name, args, prompted) = inner(command);
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    let cluster = |letter: char| {
        args.iter().any(|arg| {
            arg.len() > 1 && arg.starts_with('-') && !arg.starts_with("--") && arg.contains(letter)
        })
    };
    let direction = args.iter().find_map(|arg| {
        DIRECTIONS
            .iter()
            .position(|(flag, _)| flag == arg)
            .map(|slot| (slot as u8, DIRECTIONS[slot].1))
    });
    let kind = args
        .iter()
        .position(|arg| arg == "--kind")
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
        .or_else(|| args.iter().find_map(|arg| arg.strip_prefix("--kind=")))
        .filter(|kind| !matches!(*kind, "terminal" | "picker"));
    Some(match name.as_str() {
        "split-window" => {
            let (rank, side) = if has("-h") { (0, "right") } else { (1, "down") };
            match kind {
                Some(kind) => class(Panes, rank, false, format!("Split {side} ({kind})")),
                None => class(Panes, rank, true, format!("Split {side}")),
            }
        }
        "select-pane" if prompted && has("-T") => class(Panes, 19, false, "Change the pane title"),
        "select-pane" if has("-m") => class(Panes, 17, false, "Mark pane"),
        "select-pane" if has("-M") => class(Panes, 18, false, "Clear marked pane"),
        "select-pane" if args.iter().any(|arg| arg.ends_with(".+")) => {
            class(Panes, 5, true, "Next pane")
        }
        "select-pane" if args.len() == 1 => match direction {
            Some((slot, side)) => member(
                Panes,
                2,
                true,
                format!("Focus pane {side}"),
                ("focus", "Focus pane", slot),
            ),
            None => return None,
        },
        "resize-pane" if has("-Z") => class(Panes, 3, true, "Zoom pane"),
        "resize-pane" => {
            let (slot, side) = direction?;
            let cells = args
                .iter()
                .find(|arg| arg.parse::<u16>().is_ok())
                .map_or("1", String::as_str);
            let (rank, suffix) = if cells == "1" {
                (7, String::new())
            } else {
                (8, format!(" by {cells}"))
            };
            member(
                Panes,
                rank,
                false,
                format!("Resize pane {side}{suffix}"),
                (
                    format!("resize {cells}"),
                    format!("Resize pane{suffix}"),
                    slot,
                ),
            )
        }
        "kill-pane" => class(Panes, 4, true, "Kill pane"),
        "last-pane" => class(Panes, 6, true, "Last pane"),
        "break-pane" => class(Panes, 9, false, "Break pane to a window"),
        "swap-pane" if has("-U") || has("-D") => {
            let (slot, side) = if has("-U") { (0, "up") } else { (1, "down") };
            member(
                Panes,
                10,
                false,
                format!("Swap pane {side}"),
                ("swap", "Swap pane", slot),
            )
        }
        "display-panes" => class(Panes, 11, false, "Show pane numbers"),
        "new-pane" => class(Panes, 20, false, "New floating pane"),
        "if-shell" if args.iter().any(|arg| arg.contains("pane_floating_flag")) => {
            class(Panes, 21, false, "Float or tile pane")
        }
        "switch-client" if has("-T") => class(Panes, 22, false, "Move a floating pane"),
        "rotate-window" if has("-D") => class(Panes, 13, false, "Rotate panes back"),
        "rotate-window" => class(Panes, 12, false, "Rotate panes"),
        "next-layout" => class(Panes, 14, false, "Next layout"),
        "select-layout" if has("-E") => class(Panes, 15, false, "Spread panes evenly"),
        "select-layout" => {
            let layout = args.iter().find(|arg| !arg.starts_with('-'))?;
            let slot = LAYOUTS.iter().position(|name| name == layout)?;
            member(
                Panes,
                16,
                false,
                format!("Layout {layout}"),
                ("layout", "Layouts", slot as u8),
            )
        }
        "new-window" => class(Windows, 0, true, "New window"),
        "next-window" | "previous-window" => {
            let next = name == "next-window";
            let slot = u8::from(!next);
            let way = if next { "Next" } else { "Previous" };
            if has("-a") {
                member(
                    Windows,
                    7,
                    false,
                    format!("{way} window with alert"),
                    ("alert", "Next / previous alert", slot),
                )
            } else {
                member(
                    Windows,
                    1,
                    true,
                    format!("{way} window"),
                    ("cycle", "Next / previous window", slot),
                )
            }
        }
        "select-window" if prompted => class(Windows, 8, false, "Go to window by index"),
        "select-window" => {
            let target = args
                .iter()
                .position(|arg| arg == "-t")
                .and_then(|index| args.get(index + 1))?;
            let index: u8 = target.trim_start_matches([':', '=']).parse().ok()?;
            member(
                Windows,
                2,
                true,
                format!("Go to window {index}"),
                ("index", "Go to window", index),
            )
        }
        "choose-tree" if cluster('w') => class(Windows, 3, true, "Pick window"),
        "choose-tree" => class(Sessions, 0, true, "Pick session"),
        "last-window" => class(Windows, 4, true, "Last window"),
        "rename-window" => class(Windows, 5, true, "Rename window"),
        "kill-window" => class(Windows, 6, true, "Kill window"),
        "find-window" => class(Windows, 9, false, "Find window"),
        "move-window" => class(Windows, 10, false, "Move window"),
        "detach-client" if has("-a") => class(Sessions, 6, false, "Detach other clients"),
        "detach-client" => class(Sessions, 1, true, "Detach"),
        "rename-session" => class(Sessions, 2, true, "Rename session"),
        "new-session" => class(Sessions, 3, true, "New session"),
        "switch-client" if has("-n") || has("-p") => {
            let (slot, way) = if has("-n") {
                (0, "Next")
            } else {
                (1, "Previous")
            };
            member(
                Sessions,
                4,
                false,
                format!("{way} session"),
                ("session", "Next / previous session", slot),
            )
        }
        "switch-client" if has("-l") => class(Sessions, 5, false, "Last session"),
        "choose-client" => class(Sessions, 7, false, "Pick a client"),
        "kill-session" => class(Sessions, 8, false, "Kill session"),
        "copy-mode" if has("-u") => class(CopyPaste, 2, false, "Copy mode, page up"),
        "copy-mode" => class(CopyPaste, 0, true, "Copy mode"),
        "paste-buffer" => class(CopyPaste, 1, true, "Paste"),
        "choose-buffer" => class(CopyPaste, 3, false, "Pick a paste buffer"),
        "list-buffers" => class(CopyPaste, 4, false, "List paste buffers"),
        "delete-buffer" => class(CopyPaste, 5, false, "Delete a paste buffer"),
        "command-prompt" => class(Other, 0, true, "Command prompt"),
        "choose-path" => class(Other, 1, true, "Insert a path"),
        "list-keys" => class(Other, 2, false, "All keys"),
        "reload-config" | "source-file" => class(Other, 3, false, "Reload config"),
        "send-last-output" => class(Other, 4, false, "Send output to an agent"),
        "display-message" if args.is_empty() => class(Other, 5, false, "Show pane info"),
        "show-messages" => class(Other, 6, false, "Show messages"),
        "send-prefix" => class(Other, 7, false, "Send the prefix"),
        "clock-mode" => class(Other, 8, false, "Clock"),
        _ => return None,
    })
}

struct Entry {
    order: usize,
    key: String,
    group: Option<WhichKeyGroup>,
    rank: u8,
    core: bool,
    yours: bool,
    repeat: bool,
    label: String,
    family: Option<Family>,
}

#[must_use]
pub fn rows(tables: &[KeyTableSnapshot], table: &str, prefix: &str) -> Vec<WhichKeyRow> {
    build_rows(tables, table, prefix, true)
}

/// Like [`rows`], but never merges different commands into one row, so each
/// row can run its own first key.
#[must_use]
pub fn action_rows(tables: &[KeyTableSnapshot], table: &str, prefix: &str) -> Vec<WhichKeyRow> {
    build_rows(tables, table, prefix, false)
}

fn build_rows(
    tables: &[KeyTableSnapshot],
    table: &str,
    prefix: &str,
    families: bool,
) -> Vec<WhichKeyRow> {
    let Some(bindings) = tables.iter().find(|snapshot| snapshot.name == table) else {
        return Vec::new();
    };
    let grouped = table == "prefix";
    let stock_bindings = if grouped {
        let mut baseline = KeyTables::default();
        baseline.set_prefix(prefix);
        baseline
            .snapshot()
            .into_iter()
            .find(|snapshot| snapshot.name == "prefix")
            .map(|snapshot| snapshot.bindings)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let entries = bindings
        .bindings
        .iter()
        .filter(|binding| !is_mouse_key(&binding.key) && binding.key != "None")
        .enumerate()
        .map(|(order, binding)| {
            let stock = stock_bindings.iter().find(|stock| stock.key == binding.key);
            let yours = grouped
                && !stock.is_some_and(|stock| {
                    stock.repeat == binding.repeat && stock.commands == binding.commands
                });
            let note = binding
                .note
                .as_deref()
                .filter(|note| !note.is_empty())
                .filter(|note| stock.and_then(|stock| stock.note.as_deref()) != Some(*note));
            let class = classify(&binding.commands);
            let group = grouped.then(|| {
                class
                    .as_ref()
                    .map_or(WhichKeyGroup::Yours, |class| class.group)
            });
            let (rank, core) = class
                .as_ref()
                .map_or((u8::MAX, false), |class| (class.rank, class.core));
            let (label, family) = match (note, class) {
                (Some(note), _) => (note.to_owned(), None),
                (None, Some(class)) => (class.label, class.family),
                (None, None) => (raw_label(binding), None),
            };
            Entry {
                order,
                key: binding.key.clone(),
                group,
                rank,
                core: core || yours,
                yours,
                repeat: binding.repeat,
                label,
                family: family.filter(|_| families),
            }
        });
    let mut rows = merge(entries);
    if grouped {
        rows.sort_by_key(|(rank, row)| (row.group, *rank));
    }
    rows.into_iter().map(|(_, row)| row).collect()
}

fn merge(entries: impl Iterator<Item = Entry>) -> Vec<(u8, WhichKeyRow)> {
    let mut families: Vec<(Entry, Vec<(u8, String)>, bool)> = Vec::new();
    for entry in entries {
        let slot = entry.family.as_ref().map(|family| family.slot);
        let joined = entry.family.as_ref().and_then(|family| {
            families.iter_mut().find(|(head, _, _)| {
                head.group == entry.group
                    && head.yours == entry.yours
                    && head
                        .family
                        .as_ref()
                        .is_some_and(|other| other.id == family.id)
                    && key_shape(&head.key) == key_shape(&entry.key)
            })
        });
        if let Some((head, members, repeat)) = joined {
            members.push((slot.unwrap_or(0), entry.key));
            *repeat &= entry.repeat;
            head.core |= entry.core;
        } else {
            let members = vec![(slot.unwrap_or(0), entry.key.clone())];
            let repeat = entry.repeat;
            families.push((entry, members, repeat));
        }
    }
    let mut rows: Vec<(usize, u8, WhichKeyRow)> = Vec::new();
    for (head, mut members, repeat) in families {
        members.sort_by_key(|(slot, _)| *slot);
        let label = match &head.family {
            Some(family) if members.len() > 1 => family.label.clone(),
            _ => head.label,
        };
        let set = WhichKeySet {
            keys: members.into_iter().map(|(_, key)| key).collect(),
            yours: head.yours,
        };
        match rows
            .iter_mut()
            .find(|(_, _, row)| row.group == head.group && row.label == label)
        {
            Some((order, rank, row)) => {
                if head.yours {
                    row.keys.insert(0, set);
                } else {
                    row.keys.push(set);
                }
                row.repeat &= repeat;
                row.core |= head.core;
                *order = (*order).min(head.order);
                *rank = (*rank).min(head.rank);
            }
            None => rows.push((
                head.order,
                head.rank,
                WhichKeyRow {
                    keys: vec![set],
                    label,
                    group: head.group,
                    repeat,
                    core: head.core,
                },
            )),
        }
    }
    rows.sort_by_key(|(order, _, _)| *order);
    rows.into_iter().map(|(_, rank, row)| (rank, row)).collect()
}

fn key_shape(key: &str) -> (&str, bool) {
    let mut rest = key;
    while rest.len() > 2 && matches!(rest.get(..2), Some("C-" | "M-" | "S-")) {
        rest = &rest[2..];
    }
    (&key[..key.len() - rest.len()], rest.chars().count() == 1)
}

fn is_feedback(command: &CommandInvocation) -> bool {
    canonical_command(&command.name) == "display-message"
}

fn raw_label(binding: &KeyBindingSnapshot) -> String {
    binding
        .commands
        .iter()
        .enumerate()
        .filter(|(index, command)| *index == 0 || !is_feedback(command))
        .map(|(_, command)| {
            std::iter::once(command.name.as_str())
                .chain(command.args.iter().map(|arg| &**arg))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" ; ")
}

fn is_mouse_key(key: &str) -> bool {
    key.contains("Mouse") || key.contains("Wheel") || key.contains("Click")
}

#[cfg(test)]
mod tests {
    use zz_protocol::{Binding, CommandInvocation};

    use super::*;

    fn default_tables() -> KeyTables {
        KeyTables::default()
    }

    fn find<'a>(rows: &'a [WhichKeyRow], key: &str) -> &'a WhichKeyRow {
        rows.iter()
            .find(|row| row.keys.iter().any(|set| set.keys.iter().any(|k| k == key)))
            .unwrap_or_else(|| panic!("no row for {key}"))
    }

    fn keys(row: &WhichKeyRow) -> Vec<Vec<&str>> {
        row.keys
            .iter()
            .map(|set| set.keys.iter().map(String::as_str).collect())
            .collect()
    }

    fn binding(command: &str, repeat: bool, note: Option<&str>) -> Binding {
        let mut parts = command.split_whitespace();
        Binding {
            commands: vec![CommandInvocation::new(
                parts.next().expect("command name"),
                parts,
            )],
            repeat,
            note: note.map(str::to_owned),
        }
    }

    #[test]
    fn every_default_prefix_binding_is_classified() {
        let snapshot = default_tables().snapshot();
        let prefix = snapshot
            .iter()
            .find(|table| table.name == "prefix")
            .expect("default prefix table");
        for binding in &prefix.bindings {
            assert!(
                classify(&binding.commands).is_some(),
                "stock prefix key {:?} is not classified",
                binding.key
            );
        }
    }

    #[test]
    fn stock_families_collapse_into_one_row_each() {
        let rows = rows(&default_tables().snapshot(), "prefix", "C-b");
        assert!(rows.iter().all(|row| !row.yours()));
        let windows = find(&rows, "0");
        assert_eq!(windows.label, "Go to window");
        assert_eq!(
            keys(windows),
            [["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]]
        );
        let focus = find(&rows, "Up");
        assert_eq!(focus.label, "Focus pane");
        assert_eq!(keys(focus), [["Left", "Down", "Up", "Right"]]);
        assert!(focus.repeat);
        assert_eq!(
            keys(find(&rows, "C-Up")),
            [["C-Left", "C-Down", "C-Up", "C-Right"]]
        );
        assert_eq!(find(&rows, "M-Up").label, "Resize pane by 5");
        assert_eq!(keys(find(&rows, "n")), [["n", "p"]]);
        assert_eq!(find(&rows, "M-1").keys[0].keys.len(), 7);
        assert_eq!(find(&rows, "x").label, "Kill pane");
        assert_eq!(find(&rows, "$").label, "Rename session");
        assert_eq!(find(&rows, "'").label, "Go to window by index");
        assert_eq!(find(&rows, "C-b").label, "Send the prefix");
        assert_eq!(find(&rows, "T").label, "Change the pane title");
        assert_eq!(find(&rows, "*").label, "New floating pane");
        assert_eq!(find(&rows, "Tab").label, "Switch to a window");
        assert_eq!(find(&rows, "BTab").label, "Switch to a session");
        assert_eq!(rows.len(), 56);
    }

    #[test]
    fn action_rows_keep_each_command_on_its_own_row() {
        let rows = action_rows(&default_tables().snapshot(), "prefix", "C-b");
        assert_eq!(keys(find(&rows, "n")), [["n"]]);
        assert_eq!(keys(find(&rows, "p")), [["p"]]);
        assert_ne!(find(&rows, "n").label, find(&rows, "p").label);
        assert_eq!(keys(find(&rows, "Up")), [["Up"]]);
        assert_eq!(keys(find(&rows, "3")), [["3"]]);
    }

    #[test]
    fn rows_sort_by_group_then_rank_and_core_is_short() {
        let rows = rows(&default_tables().snapshot(), "prefix", "C-b");
        let ranks: Vec<_> = rows.iter().map(|row| row.group).collect();
        let mut sorted = ranks.clone();
        sorted.sort();
        assert_eq!(ranks, sorted);
        assert_eq!(rows[0].label, "Split right");
        assert_eq!(rows[1].label, "Split down");
        let core: Vec<_> = rows.iter().filter(|row| row.core).collect();
        assert!((15..=24).contains(&core.len()), "{}", core.len());
        for key in [
            "%", "\"", "Up", "z", "c", "n", "0", "w", "s", "d", "[", "]", ":", "F",
        ] {
            assert!(find(&rows, key).core, "{key} should be core");
        }
        for key in ["M-1", "C-Up", "(", "~", "i", "m", "?", "C-b"] {
            assert!(!find(&rows, key).core, "{key} should not be core");
        }
    }

    #[test]
    fn changed_prefix_keeps_send_prefix_stock() {
        let mut tables = default_tables();
        tables.set_prefix("C-a");
        let rows = rows(&tables.snapshot(), "prefix", "C-a");
        let send = find(&rows, "C-a");
        assert!(!send.yours());
        assert_eq!(send.group, Some(WhichKeyGroup::Other));
        assert!(rows.iter().all(|row| row.first_key() != "C-b"));
        assert!(rows.iter().all(|row| !row.yours()));
    }

    #[test]
    fn prefix_on_a_stock_key_keeps_that_key_stock() {
        let mut tables = default_tables();
        tables.set_prefix("C-o");
        let rows = rows(&tables.snapshot(), "prefix", "C-o");
        assert!(rows.iter().all(|row| !row.yours()));
    }

    #[test]
    fn none_prefix_shows_no_row() {
        let mut tables = default_tables();
        tables.set_prefix("None");
        let rows = rows(&tables.snapshot(), "prefix", "None");
        assert!(rows.iter().all(|row| row.first_key() != "None"));
        assert!(rows.iter().all(|row| !row.yours()));
    }

    #[test]
    fn stale_prefix_makes_send_prefix_yours() {
        let rows = rows(&default_tables().snapshot(), "prefix", "C-a");
        let send = find(&rows, "C-b");
        assert!(send.yours());
        assert!(send.core);
        assert_eq!(send.group, Some(WhichKeyGroup::Other));
    }

    #[test]
    fn your_bindings_join_their_group_and_merge_with_stock_aliases() {
        let mut tables = default_tables();
        tables.bind(
            "prefix",
            "c",
            binding("new-window -c #{pane_current_path}", false, None),
        );
        tables.bind(
            "prefix",
            "C-c",
            binding("new-window -c #{pane_current_path}", false, None),
        );
        tables.bind(
            "prefix",
            "|",
            binding("splitw -h -c #{pane_current_path}", false, None),
        );
        for (key, flag) in [("h", "-L"), ("j", "-D"), ("k", "-U"), ("l", "-R")] {
            tables.bind(
                "prefix",
                key,
                binding(&format!("select-pane {flag}"), false, None),
            );
        }
        tables.bind(
            "prefix",
            "Up",
            binding("select-pane -U", false, Some("Select the pane above")),
        );
        tables.bind(
            "prefix",
            "y",
            binding("zz-unknown-command a b", false, None),
        );
        let chained = |first: &str, second: &str| Binding {
            commands: vec![
                CommandInvocation::new(first.split(' ').next().unwrap(), first.split(' ').skip(1)),
                CommandInvocation::new("display-message", [second]),
            ],
            repeat: false,
            note: None,
        };
        tables.bind(
            "prefix",
            "r",
            chained("source-file ~/.tmux.conf", "Reloaded!"),
        );
        tables.bind("prefix", "b", chained("set-option -g status", "toggled"));
        let rows = rows(&tables.snapshot(), "prefix", "C-b");
        let reload = find(&rows, "r");
        assert_eq!(reload.label, "Reload config");
        assert!(reload.yours() && reload.core);
        let status = find(&rows, "b");
        assert_eq!(status.group, Some(WhichKeyGroup::Yours));
        assert_eq!(status.label, "set-option -g status");
        let new_window = find(&rows, "c");
        assert_eq!(new_window.group, Some(WhichKeyGroup::Windows));
        assert_eq!(new_window.label, "New window");
        assert_eq!(keys(new_window), [["c"], ["C-c"]]);
        assert!(new_window.keys.iter().all(|set| set.yours));
        let split = find(&rows, "|");
        assert_eq!(split.label, "Split right");
        assert_eq!(keys(split), [["|"], ["%"]]);
        assert!(split.keys[0].yours && !split.keys[1].yours);
        let focus = find(&rows, "h");
        assert_eq!(
            keys(focus),
            [vec!["h", "j", "k", "l"], vec!["Left", "Down", "Right"]]
        );
        assert!(!focus.repeat);
        let up = find(&rows, "Up");
        assert_eq!(up.label, "Select the pane above");
        assert!(up.yours());
        let y = find(&rows, "y");
        assert_eq!(y.group, Some(WhichKeyGroup::Yours));
        assert_eq!(y.label, "zz-unknown-command a b");
        assert!(y.core);
        assert_eq!(
            rows.last().map(|row| row.group),
            Some(Some(WhichKeyGroup::Yours))
        );
    }

    #[test]
    fn a_note_you_set_wins_over_the_short_label() {
        let mut tables = default_tables();
        tables.update_binding_metadata("prefix", "c", Some("Make a window".to_owned()), false);
        tables.bind("prefix", "n", binding("next-window", false, Some("Onward")));
        let rows = rows(&tables.snapshot(), "prefix", "C-b");
        let c = find(&rows, "c");
        assert!(!c.yours());
        assert_eq!(c.group, Some(WhichKeyGroup::Windows));
        assert_eq!(c.label, "Make a window");
        let n = find(&rows, "n");
        assert!(!n.yours());
        assert_eq!(n.label, "Onward");
        assert_eq!(keys(n), [["n"]]);
        assert_eq!(find(&rows, "p").label, "Previous window");
    }

    #[test]
    fn opens_all_keys_matches_only_the_stock_help_binding() {
        let list = |args: &[&str]| vec![CommandInvocation::new("list-keys", args.iter().copied())];
        assert!(opens_all_keys(&list(&["-N"])));
        assert!(opens_all_keys(&[CommandInvocation::new("lsk", ["-N"])]));
        assert!(!opens_all_keys(&list(&[])));
        assert!(!opens_all_keys(&list(&["-N", "-T", "copy-mode"])));
    }

    #[test]
    fn custom_table_is_one_flat_list() {
        let mut tables = default_tables();
        tables.bind(
            "resize",
            "h",
            binding("resize-pane -L", true, Some("Shrink left")),
        );
        tables.bind("resize", "j", binding("resize-pane -D", true, None));
        tables.bind("resize", "l", binding("resize-pane -R", true, None));
        tables.bind(
            "resize",
            "MouseDown1Pane",
            binding("select-pane", false, None),
        );
        let rows = rows(&tables.snapshot(), "resize", "C-b");
        assert_eq!(
            rows,
            vec![
                WhichKeyRow {
                    keys: vec![WhichKeySet {
                        keys: vec!["h".to_owned()],
                        yours: false,
                    }],
                    label: "Shrink left".to_owned(),
                    group: None,
                    repeat: true,
                    core: false,
                },
                WhichKeyRow {
                    keys: vec![WhichKeySet {
                        keys: vec!["j".to_owned(), "l".to_owned()],
                        yours: false,
                    }],
                    label: "Resize pane".to_owned(),
                    group: None,
                    repeat: true,
                    core: false,
                },
            ]
        );
    }

    #[test]
    fn mouse_keys_are_dropped() {
        let mut tables = default_tables();
        for key in [
            "MouseDown1Pane",
            "M-WheelUpPane",
            "DoubleClick1Pane",
            "SecondClick3Status",
        ] {
            tables.bind("prefix", key, binding("select-pane", false, None));
        }
        let snapshot = tables.snapshot();
        let all = |rows: Vec<WhichKeyRow>| {
            rows.iter()
                .flat_map(|row| row.keys.iter().flat_map(|set| set.keys.clone()))
                .collect::<Vec<_>>()
        };
        assert!(
            all(rows(&snapshot, "prefix", "C-b"))
                .iter()
                .all(|key| !is_mouse_key(key))
        );
        assert!(
            all(rows(&snapshot, "root", "C-b"))
                .iter()
                .all(|key| !is_mouse_key(key))
        );
    }

    #[test]
    fn missing_table_gives_no_rows() {
        assert!(rows(&default_tables().snapshot(), "nope", "C-b").is_empty());
    }
}
