use zz_protocol::{KeyBindingSnapshot, KeyTableSnapshot, KeyTables, catalog_command_spec};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WhichKeyGroup {
    Panes,
    Windows,
    Sessions,
    CopyPaste,
    Other,
}

impl WhichKeyGroup {
    pub const ORDER: [Self; 5] = [
        Self::Panes,
        Self::Windows,
        Self::Sessions,
        Self::CopyPaste,
        Self::Other,
    ];

    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Panes => "Panes",
            Self::Windows => "Windows",
            Self::Sessions => "Sessions",
            Self::CopyPaste => "Copy and paste",
            Self::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhichKeyRow {
    pub key: String,
    pub label: String,
    pub group: Option<WhichKeyGroup>,
    pub repeat: bool,
    pub yours: bool,
}

#[must_use]
pub fn stock_prefix_group(key: &str) -> Option<WhichKeyGroup> {
    use WhichKeyGroup::{CopyPaste, Other, Panes, Sessions, Windows};
    Some(match key {
        "%" | "\"" | "!" | "o" | "C-o" | "M-o" | " " | "E" | "M-1" | "M-2" | "M-3" | "M-4"
        | "M-5" | "M-6" | "M-7" | "z" | ";" | "{" | "}" | "q" | "m" | "M" | "x" | "Up" | "Down"
        | "Left" | "Right" | "M-Up" | "M-Down" | "M-Left" | "M-Right" | "C-Up" | "C-Down"
        | "C-Left" | "C-Right" => Panes,
        "c" | "n" | "p" | "l" | "w" | "&" | "," | "f" | "." | "'" | "M-n" | "M-p" | "0" | "1"
        | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => Windows,
        "$" | "(" | ")" | "L" | "D" | "d" | "s" => Sessions,
        "[" | "]" | "#" | "-" | "=" | "PPage" | "F" => CopyPaste,
        ":" | "?" | "i" | "~" | "r" | "e" | "C-b" => Other,
        _ => return None,
    })
}

#[must_use]
pub fn rows(tables: &[KeyTableSnapshot], table: &str, prefix: &str) -> Vec<WhichKeyRow> {
    let Some(bindings) = tables.iter().find(|snapshot| snapshot.name == table) else {
        return Vec::new();
    };
    let shown = bindings
        .bindings
        .iter()
        .filter(|binding| !is_mouse_key(&binding.key));
    if table != "prefix" {
        return shown.map(|binding| row(binding, None, false)).collect();
    }
    let mut baseline = KeyTables::default();
    baseline.set_prefix(prefix);
    let baseline_prefix = baseline.prefix().to_owned();
    let baseline = baseline.snapshot();
    let stock_bindings = baseline
        .iter()
        .find(|snapshot| snapshot.name == "prefix")
        .map_or(&[][..], |snapshot| snapshot.bindings.as_slice());
    let mut rows: Vec<WhichKeyRow> = shown
        .map(|binding| {
            let stock = stock_bindings.iter().any(|stock| {
                stock.key == binding.key
                    && stock.repeat == binding.repeat
                    && stock.commands == binding.commands
            });
            if !stock {
                return row(binding, None, true);
            }
            let group_key = if binding.key == baseline_prefix {
                "C-b"
            } else {
                binding.key.as_str()
            };
            let group = stock_prefix_group(group_key).unwrap_or(WhichKeyGroup::Other);
            row(binding, Some(group), false)
        })
        .collect();
    rows.sort_by_key(|row| (row.yours, row.group));
    rows
}

fn row(binding: &KeyBindingSnapshot, group: Option<WhichKeyGroup>, yours: bool) -> WhichKeyRow {
    WhichKeyRow {
        key: binding.key.clone(),
        label: label(binding),
        group,
        repeat: binding.repeat,
        yours,
    }
}

fn label(binding: &KeyBindingSnapshot) -> String {
    if let Some(note) = binding.note.as_deref().filter(|note| !note.is_empty()) {
        return note.to_owned();
    }
    if let Some(spec) = binding
        .commands
        .first()
        .and_then(|command| catalog_command_spec(&command.name))
    {
        return spec.description.to_owned();
    }
    binding
        .commands
        .iter()
        .map(|command| {
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
            .find(|row| row.key == key)
            .unwrap_or_else(|| panic!("no row for {key}"))
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
    fn every_default_prefix_key_has_a_group() {
        let snapshot = default_tables().snapshot();
        let prefix = snapshot
            .iter()
            .find(|table| table.name == "prefix")
            .expect("default prefix table");
        for binding in &prefix.bindings {
            assert!(
                stock_prefix_group(&binding.key).is_some(),
                "stock prefix key {:?} has no group",
                binding.key
            );
        }
        assert_eq!(stock_prefix_group("F"), Some(WhichKeyGroup::CopyPaste));
    }

    #[test]
    fn stock_prefix_rows_are_grouped_in_order() {
        let snapshot = default_tables().snapshot();
        let rows = rows(&snapshot, "prefix", "C-b");
        assert!(rows.iter().all(|row| !row.yours && row.group.is_some()));
        assert_eq!(find(&rows, "%").group, Some(WhichKeyGroup::Panes));
        assert_eq!(find(&rows, "c").group, Some(WhichKeyGroup::Windows));
        assert_eq!(find(&rows, "d").group, Some(WhichKeyGroup::Sessions));
        assert_eq!(find(&rows, "[").group, Some(WhichKeyGroup::CopyPaste));
        assert_eq!(find(&rows, "C-b").group, Some(WhichKeyGroup::Other));
        assert!(find(&rows, "Up").repeat);
        let ranks: Vec<_> = rows.iter().map(|row| row.group).collect();
        let mut sorted = ranks.clone();
        sorted.sort();
        assert_eq!(ranks, sorted);
        assert_eq!(find(&rows, "d").label, "Detach the current client");
        assert_eq!(find(&rows, "c").label, "Create a window");
    }

    #[test]
    fn changed_prefix_keeps_send_prefix_stock() {
        let mut tables = default_tables();
        tables.set_prefix("C-a");
        let rows = rows(&tables.snapshot(), "prefix", "C-a");
        let send = find(&rows, "C-a");
        assert!(!send.yours);
        assert_eq!(send.group, Some(WhichKeyGroup::Other));
        assert!(rows.iter().all(|row| row.key != "C-b"));
        assert!(rows.iter().all(|row| !row.yours));
    }

    #[test]
    fn stale_prefix_makes_send_prefix_yours() {
        let rows = rows(&default_tables().snapshot(), "prefix", "C-a");
        let send = find(&rows, "C-b");
        assert!(send.yours);
        assert_eq!(send.group, None);
        assert!(rows.last().is_some_and(|row| row.yours));
    }

    #[test]
    fn rebinding_a_stock_key_is_yours() {
        let mut tables = default_tables();
        tables.bind(
            "prefix",
            "c",
            binding("new-window -c #{pane_current_path}", false, None),
        );
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
        let rows = rows(&tables.snapshot(), "prefix", "C-b");
        let c = find(&rows, "c");
        assert!(c.yours);
        assert_eq!(c.group, None);
        assert!(find(&rows, "Up").yours);
        let y = find(&rows, "y");
        assert!(y.yours);
        assert_eq!(y.label, "zz-unknown-command a b");
        let first_yours = rows.iter().position(|row| row.yours).expect("yours rows");
        assert!(rows[first_yours..].iter().all(|row| row.yours));
    }

    #[test]
    fn notes_are_ignored_in_the_diff() {
        let mut tables = default_tables();
        tables.update_binding_metadata("prefix", "c", Some("Make a window".to_owned()), false);
        tables.bind("prefix", "n", binding("next-window", false, Some("Onward")));
        let rows = rows(&tables.snapshot(), "prefix", "C-b");
        let c = find(&rows, "c");
        assert!(!c.yours);
        assert_eq!(c.group, Some(WhichKeyGroup::Windows));
        assert_eq!(c.label, "Make a window");
        assert!(!find(&rows, "n").yours);
    }

    #[test]
    fn custom_table_is_one_flat_list() {
        let mut tables = default_tables();
        tables.bind(
            "resize",
            "h",
            binding("resize-pane -L", true, Some("Shrink left")),
        );
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
                    key: "h".to_owned(),
                    label: "Shrink left".to_owned(),
                    group: None,
                    repeat: true,
                    yours: false,
                },
                WhichKeyRow {
                    key: "l".to_owned(),
                    label: catalog_command_spec("resize-pane")
                        .expect("resize-pane spec")
                        .description
                        .to_owned(),
                    group: None,
                    repeat: true,
                    yours: false,
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
        let prefix_rows = rows(&snapshot, "prefix", "C-b");
        assert!(prefix_rows.iter().all(|row| !is_mouse_key(&row.key)));
        assert!(
            rows(&snapshot, "root", "C-b")
                .iter()
                .all(|row| !is_mouse_key(&row.key))
        );
    }

    #[test]
    fn missing_table_gives_no_rows() {
        assert!(rows(&default_tables().snapshot(), "nope", "C-b").is_empty());
    }
}
