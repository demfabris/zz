use gpui::{AnyElement, Entity, MouseButton, prelude::*};
use zz_protocol::{
    ChooseTreeAction, ChooseTreeItem, ChooseTreeKind, ChooseTreePaneKind, ChooseTreeState,
    InputMessage,
};
use zz_terminal::KeyInput;
use zz_ui::chooser::{ChooserPaneKind, chooser_subtitle, tree_chooser_row};

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

    fn item_key(item: &Self::Item) -> &str {
        &item.key
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
        chooser_subtitle(
            format!("{count} {targets} · daemon-owned"),
            state.filter_no_matches,
        )
    }

    fn row(
        item: Self::Item,
        index: usize,
        selected: bool,
        show_key_gutter: bool,
        mux: Entity<MuxClient>,
        theme: ChooserRowTheme,
    ) -> AnyElement {
        tree_row(item, index, selected, show_key_gutter, mux, theme)
    }

    fn key(input: KeyInput) -> Self::Action {
        ChooseTreeAction::Key(input)
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
    show_key_gutter: bool,
    mux: Entity<MuxClient>,
    theme: ChooserRowTheme,
) -> AnyElement {
    tree_row_element(item, index, selected, show_key_gutter, theme)
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
    show_key_gutter: bool,
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
    let (label, detail) = tree_row_text(&item);
    tree_chooser_row(
        TreeChooser::ROW_ID,
        index,
        item.key,
        show_key_gutter,
        target,
        label,
        detail,
        item.depth,
        disclosure,
        pane_kind,
        active,
        selected,
        theme,
        TERMINAL_FONT,
    )
}

fn tree_row_text(item: &ChooseTreeItem) -> (String, String) {
    if item.text.is_empty() {
        (item.label.clone(), item.detail.clone())
    } else {
        (item.text.clone(), String::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_format_replaces_the_whole_row_the_way_mode_tree_add_does() {
        let mut item = ChooseTreeItem {
            label: "dev".to_owned(),
            detail: "2 windows".to_owned(),
            target: zz_protocol::ChooseTreeTarget::Session(zz_protocol::SessionId(1)),
            depth: 0,
            flags: 0,
            pane_kind: None,
            key: "0".to_owned(),
            text: String::new(),
        };
        assert_eq!(
            tree_row_text(&item),
            ("dev".to_owned(), "2 windows".to_owned())
        );
        item.text = "<<dev>>".to_owned();
        assert_eq!(tree_row_text(&item), ("<<dev>>".to_owned(), String::new()));
    }
}
