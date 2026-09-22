use super::*;

fn chooser_item(label: &str, target: ChooseTreeTarget) -> zz_protocol::ChooseTreeItem {
    zz_protocol::ChooseTreeItem {
        label: label.to_owned(),
        detail: String::new(),
        target,
        depth: 0,
        flags: 0,
        pane_kind: None,
        key: String::new(),
        text: String::new(),
    }
}

fn window_chooser_state() -> ChooseTreeState {
    use zz_protocol::ClientId;
    let mut items = vec![
        chooser_item("dev", ChooseTreeTarget::Session(SessionId(1))),
        chooser_item("3:server", ChooseTreeTarget::Window(WindowId(10))),
        chooser_item("shell", ChooseTreeTarget::Pane(PaneId(11))),
        chooser_item("scratch", ChooseTreeTarget::Session(SessionId(2))),
        chooser_item("viewer", ChooseTreeTarget::Client(ClientId(12))),
        chooser_item("4:docs", ChooseTreeTarget::Window(WindowId(20))),
        chooser_item("plain:name", ChooseTreeTarget::Window(WindowId(21))),
    ];
    items[0].detail = "1 window".to_owned();
    items[0].flags = zz_protocol::ChooseTreeItem::ACTIVE
        | zz_protocol::ChooseTreeItem::HAS_CHILDREN
        | zz_protocol::ChooseTreeItem::EXPANDED;
    items[1].detail = "2 panes".to_owned();
    items[1].flags = zz_protocol::ChooseTreeItem::ACTIVE
        | zz_protocol::ChooseTreeItem::HAS_CHILDREN
        | zz_protocol::ChooseTreeItem::EXPANDED;
    items[1].depth = 1;
    items[2].depth = 2;
    items[2].flags = zz_protocol::ChooseTreeItem::ACTIVE;
    items[2].pane_kind = Some(zz_protocol::ChooseTreePaneKind::Terminal);
    items[1].key = "1".to_owned();
    items[3].detail = "2 windows".to_owned();
    items[3].flags =
        zz_protocol::ChooseTreeItem::HAS_CHILDREN | zz_protocol::ChooseTreeItem::EXPANDED;
    items[5].text = "<<reference>>".to_owned();
    items[5].detail = "files".to_owned();
    items[5].flags =
        zz_protocol::ChooseTreeItem::ACTIVE | zz_protocol::ChooseTreeItem::HAS_CHILDREN;
    items[5].depth = 1;
    items[6].depth = 1;
    items[5].key = "a".to_owned();
    ChooseTreeState {
        items,
        search: None,
        selected: 1,
        kind: zz_protocol::ChooseTreeKind::Windows,
        filter_no_matches: false,
        prompt: String::new(),
        help: false,
    }
}

fn chooser_indices(state: &UnifiedPalette) -> Vec<u32> {
    state
        .rows
        .iter()
        .filter_map(|entry| match entry.action {
            Some(PaletteAction::ChooseWindow(index)) => Some(index),
            _ => None,
        })
        .collect()
}

#[test]
fn daemon_tree_keeps_source_indices_and_selectable_sessions_windows_and_panes() {
    let source = window_chooser_state();
    let mut state = UnifiedPalette::new(Some(PaletteMode::Window));
    state.rebuild_window_chooser(&source, "", &MuxSnapshot::default());
    assert_eq!(chooser_indices(&state), [0, 1, 2, 3, 5, 6]);
    assert_eq!(
        state
            .rows
            .iter()
            .map(|entry| entry.row.label.as_ref())
            .collect::<Vec<_>>(),
        [
            "dev",
            "server",
            "shell",
            "scratch",
            "<<reference>>",
            "plain:name"
        ]
    );
    assert!(state.rows.iter().all(|entry| entry.action.is_some()));
    assert_eq!(state.rows[0].row.detail, "1 window");
    assert_eq!(state.rows[1].row.right, "current");
    assert_eq!(state.rows[2].row.right, "current");
    assert!(state.rows[4].row.right.is_empty());
    assert!(state.rows[4].row.detail.is_empty());
    assert!(state.rows.iter().all(|entry| entry.row.shortcut.is_none()));
    assert_eq!(state.rows[0].row.expanded, Some(true));
    assert_eq!(state.rows[1].row.expanded, Some(true));
    assert_eq!(state.rows[4].row.expanded, Some(false));
    assert_eq!(state.rows[2].row.expanded, None);
    assert_eq!(state.rows[2].row.indent, 36.0);
    assert_eq!(state.rows[2].row.icon, Some(IconName::SquareTerminal));
    assert_eq!(state.parent_index(2), Some(1));
    assert_eq!(state.parent_index(1), Some(0));
    assert_eq!(state.parent_index(3), None);
    assert_eq!(state.child_index(0), Some(1));
    assert_eq!(state.child_index(4), None);
    assert_eq!(state.initial_selection(), Some(1));

    state.grouped = false;
    state.rebuild_window_chooser(&source, "", &MuxSnapshot::default());
    assert_eq!(chooser_indices(&state), [0, 1, 2, 3, 5, 6]);
    assert_eq!(state.rows[1].row.label, "dev / server");
    assert_eq!(state.rows[2].row.label, "dev / server / shell");
    assert_eq!(state.rows[4].row.label, "scratch / <<reference>>");
    assert_eq!(state.rows[1].row.muted_prefix, "dev / ".len());
    assert!(
        state
            .rows
            .iter()
            .all(|entry| entry.row.indent == 0.0 && entry.row.expanded.is_none())
    );
}

#[test]
fn daemon_window_search_keeps_custom_text_and_matches_hidden_target_metadata() {
    let source = window_chooser_state();
    let mut state = UnifiedPalette::new(Some(PaletteMode::Window));
    for query in ["ref", "docs", "files", "@20"] {
        state.rebuild_window_chooser(&source, query, &MuxSnapshot::default());
        assert_eq!(chooser_indices(&state), [5], "query={query}");
        assert_eq!(state.rows[0].row.label, "scratch / <<reference>>");
        assert_eq!(state.rows[0].row.muted_prefix, "scratch / ".len());
    }
    state.rebuild_window_chooser(&source, "ref", &MuxSnapshot::default());
    assert!(!state.rows[0].row.matches.is_empty());
    state.rebuild_window_chooser(&source, "scratch", &MuxSnapshot::default());
    assert_eq!(chooser_indices(&state), [3, 5, 6]);
    state.rebuild_window_chooser(&source, "missing", &MuxSnapshot::default());
    assert!(state.rows.is_empty());
}

#[test]
fn daemon_session_rows_remain_selectable_without_visible_children() {
    let mut source = window_chooser_state();
    source
        .items
        .retain(|item| matches!(item.target, ChooseTreeTarget::Session(_)));
    source.items[1].text = "<<scratch>>".to_owned();
    let mut state = UnifiedPalette::new(Some(PaletteMode::Window));
    state.rebuild_window_chooser(&source, "", &MuxSnapshot::default());
    assert_eq!(chooser_indices(&state), [0, 1]);
    assert_eq!(state.rows[0].row.label, "dev");
    assert_eq!(state.rows[1].row.label, "<<scratch>>");
    state.rebuild_window_chooser(&source, "$2", &MuxSnapshot::default());
    assert_eq!(chooser_indices(&state), [1]);
}

const LOCAL: PaletteHostId = PaletteHostId(0);

fn navigation_tree() -> PaletteTree {
    let window = |id: u64, name: &str, active, panes: &[&str]| PaletteTreeWindow {
        id: WindowId(id),
        name: name.to_owned(),
        active,
        active_pane: PaneId(id * 10),
        panes: panes
            .iter()
            .enumerate()
            .map(|(index, label)| PaletteTreePane {
                id: PaneId(id * 10 + index as u64),
                label: (*label).to_owned(),
                detail: "Terminal".into(),
                icon: IconName::SquareTerminal,
                running: false,
            })
            .collect(),
    };
    PaletteTree {
        attached: LOCAL,
        hosts: vec![PaletteTreeHost {
            id: LOCAL,
            name: "local".to_owned(),
            detail: "This computer".to_owned(),
            right: "2 sessions".to_owned(),
            status: PaletteStatus::Online,
            sessions: vec![
                PaletteTreeSession {
                    id: SessionId(1),
                    name: "dev".to_owned(),
                    active: true,
                    windows: vec![
                        window(1, "editor", true, &["nvim", "cargo watch"]),
                        window(2, "logs", false, &["daemon logs"]),
                    ],
                },
                PaletteTreeSession {
                    id: SessionId(2),
                    name: "scratch".to_owned(),
                    active: false,
                    windows: vec![window(3, "experiments", false, &["python hidden"])],
                },
            ],
        }],
    }
}

fn offline_tree() -> PaletteTree {
    PaletteTree {
        attached: LOCAL,
        hosts: vec![PaletteTreeHost {
            id: LOCAL,
            name: "local".to_owned(),
            detail: "This computer".to_owned(),
            right: "offline".to_owned(),
            status: PaletteStatus::Offline,
            sessions: Vec::new(),
        }],
    }
}

fn row_labels(state: &UnifiedPalette) -> Vec<&str> {
    state
        .rows
        .iter()
        .map(|entry| entry.row.label.as_ref())
        .collect()
}

#[test]
fn default_tree_expands_current_session_and_preserves_local_branch_choices() {
    let mut state = UnifiedPalette::new(None);
    state.tree = navigation_tree();
    state.rows = state.navigation_entries("");
    assert_eq!(row_labels(&state), ["dev", "editor", "logs", "scratch"]);
    assert_eq!(state.initial_selection(), Some(1));
    assert_eq!(state.rows[0].row.expanded, Some(true));
    assert_eq!(state.rows[1].row.expanded, Some(false));
    assert_eq!(state.rows[3].row.expanded, Some(false));
    assert!(state.set_expanded(1, true));
    state.rows = state.navigation_entries("");
    assert_eq!(
        row_labels(&state),
        ["dev", "editor", "nvim", "cargo watch", "logs", "scratch"]
    );
    assert_eq!(state.rows[2].row.indent, 36.0);
    assert_eq!(state.rows[2].row.right, "current");
    assert_eq!(state.child_index(1), Some(2));
    assert_eq!(state.parent_index(2), Some(1));
    assert_eq!(state.parent_index(1), Some(0));
    assert!(!state.set_expanded(2, true));
    assert!(state.set_expanded(0, false));
    state.rows = state.navigation_entries("");
    assert_eq!(row_labels(&state), ["dev", "scratch"]);
    assert_eq!(state.initial_selection(), Some(0));
    assert!(state.set_expanded(0, true));
    state.rows = state.navigation_entries("");
    assert_eq!(
        row_labels(&state),
        ["dev", "editor", "nvim", "cargo watch", "logs", "scratch"]
    );
}

#[test]
fn navigate_entry_opens_sessions_and_search_finds_hidden_descendants() {
    let mut state = UnifiedPalette::new(Some(PaletteMode::Window));
    state.tree = navigation_tree();
    state.rows = state.navigation_entries("");
    assert_eq!(
        row_labels(&state),
        ["dev", "editor", "logs", "scratch", "experiments"]
    );
    assert_eq!(PaletteMode::Window.label(), "Navigate");
    assert_eq!(state.placeholder(), "Search sessions, windows, panes");
    assert!(state.set_expanded(3, false));
    state.rows = state.navigation_entries("python");
    assert_eq!(
        row_labels(&state),
        ["scratch / experiments / python hidden"]
    );
    assert_eq!(
        state.rows[0].row.muted_prefix,
        "scratch / experiments / ".len()
    );
    assert!(state.rows[0].row.expanded.is_none());
    assert_eq!(state.parent_index(0), None);
    assert!(!state.rows[0].row.matches.is_empty());
    state.rows = state.navigation_entries("%30");
    assert_eq!(
        row_labels(&state),
        ["scratch / experiments / python hidden"]
    );
    state.rows = state.navigation_entries("scratch");
    assert_eq!(
        row_labels(&state),
        [
            "scratch",
            "scratch / experiments",
            "scratch / experiments / python hidden"
        ]
    );
    state.rows = state.navigation_entries("");
    assert_eq!(row_labels(&state), ["dev", "editor", "logs", "scratch"]);
    state.command = zz_protocol::catalog_command_spec("join-pane");
    state.rows = state.navigation_entries("");
    assert_eq!(
        row_labels(&state),
        ["dev / editor", "dev / logs", "scratch / experiments"]
    );
    assert!(state.rows.iter().all(|entry| matches!(
        entry.action,
        Some(PaletteAction::Target {
            target: PaletteTarget::Window(_),
            ..
        })
    )));
}

#[test]
fn default_recent_commands_are_unique_and_precede_hosts() {
    let history = [
        "rename-window one",
        "rename-window two",
        "new-window",
        "list-panes",
        "kill-pane",
    ]
    .map(str::to_owned);
    let mut state = UnifiedPalette::new(None);
    state.rebuild(
        "",
        &history,
        offline_tree(),
        PaneKindAvailability::default(),
        &|_| None,
    );
    let headers = state
        .rows
        .iter()
        .filter(|entry| entry.action.is_none())
        .map(|entry| entry.row.label.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(headers, ["Recent commands", "Hosts"]);
    let commands = state
        .rows
        .iter()
        .filter_map(|entry| match entry.action {
            Some(PaletteAction::Command(spec)) => Some(spec.name),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(commands, ["rename-window", "new-window", "list-panes"]);
}

#[test]
fn command_rows_hide_unavailable_pane_actions() {
    let availability = PaneKindAvailability {
        browser: false,
        agent: false,
        editor: false,
    };
    let rows = command_entries("", availability, &|_| None);
    for hidden in [
        "capture-browser",
        "set-browser-url",
        "set-browser-tabs",
        "set-browser-profile",
        "set-editor-path",
        "agent-send",
        "restart-agent-pane",
        "set-agent-provider",
        "agent-catalog",
        "agent-respond",
        "new-agent-session",
        "select-pane-kind",
    ] {
        assert!(
            !rows.iter().any(|entry| entry.row.label == hidden),
            "{hidden}"
        );
    }
    assert!(rows.iter().any(|entry| entry.row.label == "split-window"));
}

#[test]
fn fuzzy_matching_ignores_query_spaces_and_tracks_unicode_byte_ranges() {
    assert_eq!(fuzzy_match("α 界", "aΑ-界"), Some((3, vec![1..3, 4..7])));
    assert_eq!(
        fuzzy_match("pane", "join-pane"),
        Some((5, vec![5..6, 6..7, 7..8, 8..9]))
    );
    assert!(fuzzy_match("pnx", "pane").is_none());
}

#[test]
fn backspace_pops_exactly_one_palette_layer() {
    let mut state = UnifiedPalette::new(Some(PaletteMode::Command));
    state.host = Some(LOCAL);
    state.command = zz_protocol::catalog_command_spec("join-pane");
    assert!(state.pop_layer());
    assert!(state.command.is_none());
    assert_eq!(state.mode, Some(PaletteMode::Command));
    assert!(state.pop_layer());
    assert_eq!(state.host, Some(LOCAL));
    assert!(state.pop_layer());
    assert!(!state.pop_layer());
}

#[test]
fn host_prefix_is_configurable_without_stealing_the_alternative() {
    assert_eq!(PaletteMode::from_prefix("#", "#"), Some(PaletteMode::Host));
    assert_eq!(PaletteMode::from_prefix("~", "#"), None);
    assert_eq!(
        PaletteMode::from_prefix("@", "~"),
        Some(PaletteMode::Window)
    );
}

#[test]
fn navigation_skips_headers_and_wraps_from_the_initial_selection() {
    let mut state = UnifiedPalette::new(None);
    state.rows = vec![
        PaletteEntry::header("Windows", "@"),
        PaletteEntry::row(PaletteRow::default(), PaletteAction::Host(LOCAL)),
        PaletteEntry::header("Commands", ":"),
        PaletteEntry::row(
            PaletteRow::default(),
            PaletteAction::History("list-panes".to_owned()),
        ),
    ];
    assert_eq!(state.navigate(Some(1), 1), Some(3));
    assert_eq!(state.navigate(Some(3), 1), Some(1));
    assert_eq!(state.navigate(Some(1), -1), Some(3));
    state.rows.clear();
    assert_eq!(state.navigate(None, 1), None);
}

#[test]
fn target_step_supports_join_without_forcing_optional_new_window_targets() {
    assert_eq!(
        target_kind(zz_protocol::catalog_command_spec("join-pane").unwrap()),
        Some(CommandValueKind::Window)
    );
    assert_eq!(
        target_kind(zz_protocol::catalog_command_spec("kill-pane").unwrap()),
        Some(CommandValueKind::Pane)
    );
    assert_eq!(
        target_kind(zz_protocol::catalog_command_spec("new-window").unwrap()),
        None
    );
    assert_eq!(
        target_kind(zz_protocol::catalog_command_spec("split-window").unwrap()),
        None
    );
    assert_eq!(
        target_kind(zz_protocol::catalog_command_spec("rename-window").unwrap()),
        None
    );
    assert!(!needs_arguments(
        zz_protocol::catalog_command_spec("new-window").unwrap()
    ));
    assert!(!needs_arguments(
        zz_protocol::catalog_command_spec("split-window").unwrap()
    ));
    assert!(needs_arguments(
        zz_protocol::catalog_command_spec("rename-window").unwrap()
    ));
    assert!(needs_arguments(
        zz_protocol::catalog_command_spec("if-shell").unwrap()
    ));
}
