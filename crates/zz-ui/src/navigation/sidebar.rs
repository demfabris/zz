use std::{
    rc::Rc,
    sync::{Arc, LazyLock},
};

use gpui::{
    AnyElement, App, ElementId, Hsla, IntoElement, MouseButton, SharedString, Stateful, Window,
    div, prelude::*, px,
};

use crate::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _, WindowExt as _,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    notification::Notification,
    spinner::Spinner,
    tooltip::Tooltip,
};

use super::{workspace_tree_action_button, workspace_tree_marker};

pub fn tree_host_marker(bell: bool, badge_color: Option<Hsla>, cx: &App) -> AnyElement {
    static LOGO: LazyLock<Arc<gpui::Image>> = LazyLock::new(|| {
        Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/linux/hicolor/256x256/apps/zz.png"
            ))
            .to_vec(),
        ))
    });
    tree_node_marker(
        gpui::img(Arc::clone(&LOGO))
            .size(crate::rems_from_px(super::WORKSPACE_TREE_NODE_ICON_SIZE)),
        bell,
        badge_color,
        cx,
    )
}

pub fn tree_host_indicator(
    id: impl Into<ElementId>,
    connecting: bool,
    detail: Option<SharedString>,
    cx: &App,
) -> AnyElement {
    if connecting {
        return Spinner::new()
            .xsmall()
            .color(cx.theme().foreground.muted())
            .into_any_element();
    }
    div()
        .id(id)
        .flex()
        .flex_none()
        .when_some(detail, |this, detail| {
            let toast_detail = detail.clone();
            this.cursor_pointer()
                .tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    window.push_notification(Notification::warning(toast_detail.clone()), cx);
                })
        })
        .child(Icon::new(IconName::Xmark).xsmall())
        .into_any_element()
}

pub fn tree_node_marker(
    icon: impl IntoElement,
    bell: bool,
    badge_color: Option<Hsla>,
    cx: &App,
) -> AnyElement {
    workspace_tree_marker(
        div()
            .relative()
            .flex_none()
            .child(icon)
            .when(bell, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(-1.0))
                        .right(px(-1.0))
                        .size(px(5.0))
                        .rounded_full()
                        .bg(cx.theme().warning),
                )
            })
            .when_some(badge_color, |this, color| {
                this.child(
                    div()
                        .absolute()
                        .bottom(px(-1.0))
                        .right(px(-1.0))
                        .flex_none()
                        .size(px(5.0))
                        .rounded_full()
                        .bg(color),
                )
            }),
    )
    .into_any_element()
}

pub fn tree_row_rename_menu(
    row: Stateful<gpui::Div>,
    label: SharedString,
    on_rename: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_rename = Rc::new(on_rename);
    row.context_menu(move |menu, _, _| {
        let on_rename = on_rename.clone();
        menu.item(
            PopupMenuItem::new(label.clone()).on_click(move |_, window, cx| on_rename(window, cx)),
        )
    })
    .into_any_element()
}

pub fn tree_action_strip(id: impl Into<ElementId>, actions: Vec<AnyElement>) -> AnyElement {
    div()
        .id(id)
        .h_full()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .children(actions)
        .into_any_element()
}

pub fn tree_window_layout_button(
    id: impl Into<ElementId>,
    disabled: bool,
    on_split: impl Fn(bool, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let on_split = Rc::new(on_split);
    workspace_tree_action_button(id, IconName::LayoutColumns, "Window layout", disabled, cx)
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
            [
                ("Split right", IconName::PanelRight, true),
                ("Split bottom", IconName::PanelBottom, false),
            ]
            .into_iter()
            .fold(menu, |menu, (label, icon, horizontal)| {
                let on_split = on_split.clone();
                menu.item(
                    PopupMenuItem::new(label)
                        .icon(icon)
                        .on_click(move |_, window, cx| on_split(horizontal, window, cx)),
                )
            })
        })
        .into_any_element()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeNavigationRow {
    pub depth: u8,
    pub parent: Option<usize>,
    pub expandable: bool,
    pub expanded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeNavigation {
    Up,
    Down,
    First,
    Last,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeNavigationResult {
    Select(usize),
    Toggle(usize),
    None,
}

pub fn tree_navigation(
    rows: &[TreeNavigationRow],
    selected: Option<usize>,
    direction: TreeNavigation,
) -> TreeNavigationResult {
    use TreeNavigationResult::{None, Select, Toggle};
    if rows.is_empty() {
        return None;
    }
    if selected.is_none() && matches!(direction, TreeNavigation::Left | TreeNavigation::Right) {
        return None;
    }
    let index = selected.unwrap_or(0).min(rows.len() - 1);
    let row = rows[index];
    match direction {
        TreeNavigation::Up => Select(index.saturating_sub(1)),
        TreeNavigation::Down => Select((index + 1).min(rows.len() - 1)),
        TreeNavigation::First => Select(0),
        TreeNavigation::Last => Select(rows.len() - 1),
        TreeNavigation::Left if row.expandable && row.expanded => Toggle(index),
        TreeNavigation::Left => row.parent.map_or(None, Select),
        TreeNavigation::Right if row.expandable && !row.expanded => Toggle(index),
        TreeNavigation::Right
            if rows
                .get(index + 1)
                .is_some_and(|child| child.depth == row.depth + 1) =>
        {
            Select(index + 1)
        }
        TreeNavigation::Right => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_navigation_expands_then_enters_and_collapses_then_leaves() {
        let mut rows = vec![
            TreeNavigationRow {
                depth: 0,
                parent: None,
                expandable: true,
                expanded: true,
            },
            TreeNavigationRow {
                depth: 1,
                parent: Some(0),
                expandable: true,
                expanded: false,
            },
        ];
        assert_eq!(
            tree_navigation(&rows, Some(1), TreeNavigation::Right),
            TreeNavigationResult::Toggle(1)
        );
        rows[1].expanded = true;
        rows.push(TreeNavigationRow {
            depth: 2,
            parent: Some(1),
            expandable: false,
            expanded: false,
        });
        assert_eq!(
            tree_navigation(&rows, Some(1), TreeNavigation::Right),
            TreeNavigationResult::Select(2)
        );
        assert_eq!(
            tree_navigation(&rows, Some(2), TreeNavigation::Left),
            TreeNavigationResult::Select(1)
        );
        assert_eq!(
            tree_navigation(&rows, Some(1), TreeNavigation::Left),
            TreeNavigationResult::Toggle(1)
        );
    }

    #[test]
    fn tree_navigation_stays_within_visible_rows() {
        let rows = [TreeNavigationRow {
            depth: 0,
            parent: None,
            expandable: false,
            expanded: false,
        }];
        assert_eq!(
            tree_navigation(&rows, None, TreeNavigation::Up),
            TreeNavigationResult::Select(0)
        );
        assert_eq!(
            tree_navigation(&rows, Some(0), TreeNavigation::Down),
            TreeNavigationResult::Select(0)
        );
        assert_eq!(
            tree_navigation(&[], None, TreeNavigation::First),
            TreeNavigationResult::None
        );
    }
}
