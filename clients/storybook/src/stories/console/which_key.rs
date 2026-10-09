use std::sync::Arc;

use zpui::{AnyElement, App, IntoElement, Keystroke, Window, px, size};
use zz_ui::which_key::{WhichKeyCap, WhichKeyHeader, WhichKeyRow, WhichKeyView};

use crate::story::{Section, Story, stateless, states};

pub const STORY: Story = Story {
    id: "which-key",
    name: "Which-key",
    group: "Commands",
    summary: "The sheet that appears after the prefix: every binding in the table, grouped, with your own bindings in the accent color.",
    sections: &[
        Section {
            id: "prefix",
            name: "Prefix table",
            summary: "The default prefix table with no size limit: one column per group. Digit runs fold into a range, symbol pairs into one cap, and repeatable keys carry a loop mark.",
            build: |_, cx| stateless(prefix, cx),
        },
        Section {
            id: "fit",
            name: "Fit to a box",
            summary: "WhichKeyView::fit packs small groups under each other and splits a tall group across columns to stay inside the space it is given.",
            build: |_, cx| stateless(fitted, cx),
        },
        Section {
            id: "other-tables",
            name: "Other tables",
            summary: "A key table with no prefix names itself in the header. Keys with no keystroke spelling show their tmux name.",
            build: |_, cx| stateless(other_tables, cx),
        },
    ],
};

fn cap(keys: &[&str], raw: &str, yours: bool) -> WhichKeyCap {
    WhichKeyCap {
        keys: keys
            .iter()
            .filter_map(|key| Keystroke::parse(key).ok())
            .collect(),
        raw: raw.to_owned().into(),
        yours,
    }
}

fn binding(
    id: &str,
    keys: &[&str],
    raw: &str,
    label: &str,
    group: &str,
    yours: bool,
    repeat: bool,
) -> WhichKeyRow {
    WhichKeyRow {
        id: id.to_owned().into(),
        caps: vec![cap(keys, raw, yours)],
        label: label.to_owned().into(),
        group: Some(group.to_owned().into()),
        repeat,
    }
}

fn key(id: &str, label: &str, group: &str) -> WhichKeyRow {
    binding(id, &[id], id, label, group, false, false)
}

pub(super) fn prefix_rows() -> Vec<WhichKeyRow> {
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    vec![
        key("c", "New window", "Windows"),
        binding(
            "digits",
            &DIGITS,
            "0 … 9",
            "Select window",
            "Windows",
            false,
            false,
        ),
        key("n", "Next window", "Windows"),
        key("p", "Previous window", "Windows"),
        key(",", "Rename window", "Windows"),
        key("&", "Kill window", "Windows"),
        key("w", "Choose a window", "Windows"),
        key("%", "Split left and right", "Panes"),
        key("\"", "Split top and bottom", "Panes"),
        binding(
            "arrows",
            &["left", "down", "up", "right"],
            "Left Down Up Right",
            "Select pane",
            "Panes",
            false,
            true,
        ),
        binding(
            "resize",
            &["alt-left", "alt-down", "alt-up", "alt-right"],
            "M-Left M-Down M-Up M-Right",
            "Resize pane",
            "Panes",
            false,
            true,
        ),
        binding(
            "swap",
            &["{", "}"],
            "{ }",
            "Swap pane",
            "Panes",
            false,
            false,
        ),
        key("o", "Next pane", "Panes"),
        key("z", "Zoom pane", "Panes"),
        key("x", "Kill pane", "Panes"),
        key("q", "Show pane numbers", "Panes"),
        key("d", "Detach", "Sessions"),
        key("s", "Choose a session", "Sessions"),
        key("$", "Rename session", "Sessions"),
        binding(
            "switch",
            &["(", ")"],
            "( )",
            "Previous or next session",
            "Sessions",
            false,
            false,
        ),
        key("[", "Copy mode", "Copy"),
        key("]", "Paste buffer", "Copy"),
        key("=", "Choose a buffer", "Copy"),
        binding("g", &["g"], "g", "Open lazygit", "Yours", true, false),
        binding(
            "shift-p",
            &["shift-p"],
            "P",
            "Toggle pane logging",
            "Yours",
            true,
            false,
        ),
        binding(
            "ctrl-r",
            &["ctrl-r"],
            "C-r",
            "Reload config and say so in the status line",
            "Yours",
            true,
            false,
        ),
        key(":", "Command prompt", "Other"),
        key("?", "List keys", "Other"),
        key("t", "Clock", "Other"),
    ]
}

fn prefix_header() -> WhichKeyHeader {
    WhichKeyHeader {
        table: "prefix".into(),
        prefix: Keystroke::parse("ctrl-b").ok(),
        prefix_raw: "C-b".into(),
        more: Some(cap(&["?"], "?", false)),
    }
}

fn prefix(_: &mut Window, _: &mut App) -> AnyElement {
    WhichKeyView::new(prefix_header(), prefix_rows()).into_any_element()
}

fn fitted(_: &mut Window, _: &mut App) -> AnyElement {
    let rows: Arc<[WhichKeyRow]> = prefix_rows().into();
    states()
        .state(
            "880 × 300",
            WhichKeyView::new(prefix_header(), Arc::clone(&rows)).fit(size(px(880.0), px(300.0))),
        )
        .state(
            "520 × 420",
            WhichKeyView::new(prefix_header(), Arc::clone(&rows)).fit(size(px(520.0), px(420.0))),
        )
        .state(
            "too small: 520 × 200, the body scrolls",
            WhichKeyView::new(prefix_header(), rows).fit(size(px(520.0), px(200.0))),
        )
        .into_any_element()
}

fn other_tables(_: &mut Window, _: &mut App) -> AnyElement {
    let copy = vec![
        binding(
            "hjkl",
            &["h", "j", "k", "l"],
            "h j k l",
            "Move the cursor",
            "Move",
            false,
            true,
        ),
        key("w", "Next word", "Move"),
        key("b", "Previous word", "Move"),
        binding(
            "ctrl-u",
            &["ctrl-u", "ctrl-d"],
            "C-u C-d",
            "Half page up or down",
            "Move",
            false,
            true,
        ),
        key("v", "Begin selection", "Select"),
        binding(
            "shift-v",
            &["shift-v"],
            "V",
            "Select line",
            "Select",
            false,
            false,
        ),
        key("y", "Copy and exit", "Select"),
        key("/", "Search down", "Search"),
        key("?", "Search up", "Search"),
        key("n", "Next match", "Search"),
        key("q", "Exit copy mode", "Other"),
    ];
    let root = vec![
        binding(
            "alt-hjkl",
            &["alt-h", "alt-j", "alt-k", "alt-l"],
            "M-h M-j M-k M-l",
            "Select pane",
            "Panes",
            true,
            true,
        ),
        binding(
            "alt-digits",
            &["alt-1", "alt-2", "alt-3", "alt-4", "alt-5"],
            "M-1 … M-5",
            "Select window",
            "Windows",
            true,
            false,
        ),
        binding(
            "mouse",
            &[],
            "MouseDown1Pane",
            "Select the pane under the pointer",
            "Mouse",
            false,
            false,
        ),
        binding(
            "wheel",
            &[],
            "WheelUpPane",
            "Scroll into copy mode",
            "Mouse",
            false,
            false,
        ),
        binding(
            "f12",
            &["f12"],
            "F12",
            "Toggle the prefix",
            "Other",
            true,
            false,
        ),
    ];
    states()
        .state(
            "copy-mode-vi",
            WhichKeyView::new(
                WhichKeyHeader {
                    table: "copy-mode-vi".into(),
                    prefix: None,
                    prefix_raw: "copy".into(),
                    more: Some(cap(&["?"], "?", false)),
                },
                copy,
            ),
        )
        .state(
            "root, no more keys",
            WhichKeyView::new(
                WhichKeyHeader {
                    table: "root".into(),
                    prefix: None,
                    prefix_raw: String::new().into(),
                    more: None,
                },
                root,
            ),
        )
        .into_any_element()
}
