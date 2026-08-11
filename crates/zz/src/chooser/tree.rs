use gpui::{AnyElement, Entity, Keystroke, MouseButton, prelude::*};
use zz_protocol::{
    ChooseTreeAction, ChooseTreeItem, ChooseTreeKind, ChooseTreePaneKind, ChooseTreeState,
    InputMessage,
};
use zz_ui::chooser::{ChooserPaneKind, tree_chooser_row};

use crate::{
    chooser::{Chooser, ChooserHint, ChooserRowTheme, ChooserSearch, ChooserSpec},
    mux::client::MuxClient,
    terminal::view::TERMINAL_FONT,
};

const TREE_HINTS: &[ChooserHint] = &[
    ChooserHint {
        keys: &["up", "down"],
        label: "navigate",
    },
    ChooserHint {
        keys: &["left", "right"],
        label: "collapse",
    },
    ChooserHint {
        keys: &["enter"],
        label: "choose",
    },
    ChooserHint {
        keys: &["/"],
        label: "search",
    },
    ChooserHint {
        keys: &["escape"],
        label: "close",
    },
];

#[derive(Default)]
pub(crate) struct TreeChooser;

pub(crate) type ChooseTreeView = Chooser<TreeChooser>;

impl ChooserSpec for TreeChooser {
    type State = ChooseTreeState;
    type Item = ChooseTreeItem;
    type Action = ChooseTreeAction;

    const OVERLAY_ID: &'static str = "choose-tree-overlay";
    const MODAL_ID: &'static str = "choose-tree-modal";
    const ROWS_ID: &'static str = "choose-tree-rows";
    const ROW_ID: &'static str = "choose-tree-row";
    const CLOSE_ID: &'static str = "choose-tree-close";
    const WIDTH: f32 = 0.82;
    const MAX_WIDTH: f32 = 920.0;
    const HEIGHT: f32 = 0.74;
    const MIN_HEIGHT: f32 = 300.0;
    const MAX_HEIGHT: f32 = 640.0;
    const HINTS: &'static [ChooserHint] = TREE_HINTS;

    fn state(mux: &MuxClient) -> Option<Self::State> {
        mux.choose_tree().cloned()
    }

    fn selected(state: &Self::State) -> u32 {
        state.selected
    }

    fn items(state: &Self::State) -> &[Self::Item] {
        &state.items
    }

    fn search(state: &Self::State) -> Option<ChooserSearch<'_>> {
        state.search.as_ref().map(|search| ChooserSearch {
            query: &search.query,
            reverse: search.reverse,
        })
    }

    fn title(state: &Self::State) -> &'static str {
        match state.kind {
            ChooseTreeKind::Windows => "Choose window",
            ChooseTreeKind::Panes => "Choose pane",
        }
    }

    fn subtitle(state: &Self::State, count: usize) -> String {
        let targets = match state.kind {
            ChooseTreeKind::Windows => "sessions and windows",
            ChooseTreeKind::Panes => "sessions, windows, and panes",
        };
        format!("{count} {targets} · daemon-owned")
    }

    fn row(
        item: Self::Item,
        index: usize,
        selected: bool,
        mux: Entity<MuxClient>,
        theme: ChooserRowTheme,
    ) -> AnyElement {
        tree_row(item, index, selected, mux, theme)
    }

    fn key_action(keystroke: &Keystroke, searching: bool) -> Option<Self::Action> {
        choose_tree_key_action(keystroke, searching)
    }

    fn search_active_after(action: &Self::Action) -> Option<bool> {
        match action {
            ChooseTreeAction::SearchStart { .. } => Some(true),
            ChooseTreeAction::SearchAccept | ChooseTreeAction::SearchCancel => Some(false),
            _ => None,
        }
    }

    fn search_append(text: String) -> Self::Action {
        ChooseTreeAction::SearchAppend(text)
    }

    fn close() -> Self::Action {
        ChooseTreeAction::Close
    }

    fn send(mux: &MuxClient, action: Self::Action) {
        mux.send_input(InputMessage::ChooseTree { action });
    }
}

fn tree_row(
    item: ChooseTreeItem,
    index: usize,
    selected: bool,
    mux: Entity<MuxClient>,
    theme: ChooserRowTheme,
) -> AnyElement {
    tree_row_element(item, index, selected, theme)
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            TreeChooser::send(
                mux.read(cx),
                if event.click_count >= 2 {
                    ChooseTreeAction::ActivateIndex(index)
                } else {
                    ChooseTreeAction::Select(index)
                },
            );
            cx.stop_propagation();
        })
        .into_any_element()
}

fn tree_row_element(
    item: ChooseTreeItem,
    index: usize,
    selected: bool,
    theme: ChooserRowTheme,
) -> zz_ui::list::ListItem {
    let target = item.target.to_string();
    let disclosure = if item.has_children() {
        if item.expanded() { "▾" } else { "▸" }
    } else {
        ""
    };
    let pane_kind = item.pane_kind.map(|kind| match kind {
        ChooseTreePaneKind::Terminal => ChooserPaneKind::Terminal,
        ChooseTreePaneKind::Browser => ChooserPaneKind::Browser,
        ChooseTreePaneKind::Agent => ChooserPaneKind::Agent,
        ChooseTreePaneKind::Editor => ChooserPaneKind::Editor,
    });
    let active = item.active();
    tree_chooser_row(
        TreeChooser::ROW_ID,
        index,
        target,
        item.label,
        item.detail,
        item.depth,
        disclosure,
        pane_kind,
        active,
        selected,
        theme,
        TERMINAL_FONT,
    )
}

fn choose_tree_key_action(keystroke: &Keystroke, searching: bool) -> Option<ChooseTreeAction> {
    let modifiers = keystroke.modifiers;
    let key = keystroke.key.as_str();
    let character = keystroke.key_char.as_deref().unwrap_or(key);
    if searching {
        return match key {
            "escape" => Some(ChooseTreeAction::SearchCancel),
            "enter" => Some(ChooseTreeAction::SearchAccept),
            "backspace" => Some(ChooseTreeAction::SearchBackspace),
            "up" => Some(ChooseTreeAction::Previous),
            "down" => Some(ChooseTreeAction::Next),
            "g" if modifiers.control => Some(ChooseTreeAction::SearchCancel),
            _ => None,
        };
    }

    match key {
        "escape" => Some(ChooseTreeAction::Close),
        "enter" => Some(ChooseTreeAction::Activate),
        "up" => Some(ChooseTreeAction::Previous),
        "down" => Some(ChooseTreeAction::Next),
        "left" => Some(ChooseTreeAction::Collapse),
        "right" => Some(ChooseTreeAction::Expand),
        "pageup" => Some(ChooseTreeAction::PagePrevious),
        "pagedown" => Some(ChooseTreeAction::PageNext),
        "home" => Some(ChooseTreeAction::First),
        "end" => Some(ChooseTreeAction::Last),
        "p" if modifiers.control => Some(ChooseTreeAction::Previous),
        "n" if modifiers.control => Some(ChooseTreeAction::Next),
        "b" if modifiers.control => Some(ChooseTreeAction::PagePrevious),
        "f" if modifiers.control => Some(ChooseTreeAction::PageNext),
        "s" if modifiers.control => Some(ChooseTreeAction::SearchStart { reverse: false }),
        "g" | "[" if modifiers.control => Some(ChooseTreeAction::Close),
        "q" if no_command_modifiers(modifiers) => Some(ChooseTreeAction::Close),
        "k" if no_command_modifiers(modifiers) => Some(ChooseTreeAction::Previous),
        "j" if no_command_modifiers(modifiers) => Some(ChooseTreeAction::Next),
        "h" | "-" if no_command_modifiers(modifiers) => Some(ChooseTreeAction::Collapse),
        "l" if no_command_modifiers(modifiers) => Some(ChooseTreeAction::Expand),
        "g" if no_command_modifiers(modifiers) && character != "G" => Some(ChooseTreeAction::First),
        "n" if no_command_modifiers(modifiers) => Some(ChooseTreeAction::SearchNext {
            reverse: modifiers.shift,
        }),
        _ if character == "G" => Some(ChooseTreeAction::Last),
        _ if character == "+" => Some(ChooseTreeAction::Expand),
        _ if character == "/" => Some(ChooseTreeAction::SearchStart { reverse: false }),
        _ if character == "?" => Some(ChooseTreeAction::SearchStart { reverse: true }),
        _ => None,
    }
}

fn no_command_modifiers(modifiers: gpui::Modifiers) -> bool {
    !modifiers.control && !modifiers.alt && !modifiers.platform
}

#[cfg(test)]
mod tests {
    use gpui::Modifiers;

    use super::*;

    fn key(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            key: key.to_owned(),
            key_char: key_char.map(str::to_owned),
            modifiers,
        }
    }

    #[test]
    fn tmux_tree_keys_map_to_native_actions() {
        assert_eq!(
            choose_tree_key_action(&key("j", Some("j"), Modifiers::default()), false),
            Some(ChooseTreeAction::Next)
        );
        assert_eq!(
            choose_tree_key_action(&key("/", Some("?"), Modifiers::default()), false),
            Some(ChooseTreeAction::SearchStart { reverse: true })
        );
        assert_eq!(
            choose_tree_key_action(&key("escape", None, Modifiers::default()), true),
            Some(ChooseTreeAction::SearchCancel)
        );
        assert_eq!(
            choose_tree_key_action(&key("enter", None, Modifiers::default()), false),
            Some(ChooseTreeAction::Activate)
        );
    }
}
