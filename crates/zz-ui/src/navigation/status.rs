use std::{ops::Range, rc::Rc};

use gpui::{
    AnyElement, App, ElementId, IntoElement, MouseButton, SharedString, Window, div, prelude::*, px,
};

use crate::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
};

use super::{
    WorkspaceStatusWindowState, workspace_row_highlight, workspace_status_item,
    workspace_status_window, workspace_tree_action_button,
};

pub const MAX_VISIBLE_WINDOWS: usize = 5;
pub type StatusAction = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct StatusWindowActions {
    pub select: StatusAction,
    pub close: Option<StatusAction>,
    pub rename: Option<(SharedString, StatusAction)>,
}

pub struct StatusWindowEntry {
    pub label: SharedString,
    pub active: bool,
    pub select: StatusAction,
}

pub fn visible_window_range(total: usize, active: usize, limit: usize) -> Range<usize> {
    if total <= limit || limit == 0 {
        return 0..total;
    }
    let start = active.saturating_sub(limit / 2).min(total - limit);
    start..start + limit
}

pub fn status_session(
    id: impl Into<ElementId>,
    name: SharedString,
    on_focus: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let foreground = cx.theme().foreground;
    let highlight = workspace_row_highlight(cx);
    workspace_status_item(id, Some(IconName::Layers), name, cx)
        .flex_none()
        .px(px(8.0))
        .rounded(cx.theme().radius)
        .when(cx.theme().shadow, |item| {
            item.border(px(0.5)).border_color(gpui::transparent_white())
        })
        .text_color(foreground)
        .cursor_pointer()
        .hover(move |item| {
            let item = item.bg(highlight).text_color(foreground);
            if cx.theme().shadow {
                item.control_highlight(cx)
            } else {
                item
            }
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            on_focus(window, cx);
        })
        .into_any_element()
}

pub fn status_window(
    id: impl Into<ElementId>,
    index: SharedString,
    name: SharedString,
    state: WorkspaceStatusWindowState,
    actions: StatusWindowActions,
    cx: &App,
) -> AnyElement {
    let id = id.into();
    let group: SharedString = format!("status-window-{id:?}").into();
    let close_button = actions.close.clone();
    let tooltip = format!("{index}:{name}").into();
    let item = workspace_status_window(id.clone(), index, name, tooltip, state, cx)
        .group(group.clone())
        .pr(px(0.0))
        .child(
            div()
                .flex_none()
                .invisible()
                .group_hover(group, gpui::Styled::visible)
                .child(
                    workspace_tree_action_button(
                        ElementId::Name(format!("status-window-close-{id:?}").into()),
                        IconName::Xmark,
                        "Close window",
                        !state.connected || close_button.is_none(),
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        if let Some(close) = &close_button {
                            close(window, cx);
                        }
                    }),
                ),
        )
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            if state.connected {
                (actions.select)(window, cx);
            }
        });
    if !state.connected || actions.rename.is_none() && actions.close.is_none() {
        return item.into_any_element();
    }
    item.context_menu(move |mut menu, _, _| {
        if let Some((label, rename)) = &actions.rename {
            let rename = rename.clone();
            menu = menu.item(
                PopupMenuItem::new(label.clone()).on_click(move |_, window, cx| rename(window, cx)),
            );
        }
        if let Some(close) = &actions.close {
            let close = close.clone();
            menu = menu.item(
                PopupMenuItem::new("Close Window")
                    .icon(IconName::Xmark)
                    .on_click(move |_, window, cx| close(window, cx)),
            );
        }
        menu
    })
    .into_any_element()
}

pub fn status_window_overflow(
    id: impl Into<ElementId>,
    windows: Vec<StatusWindowEntry>,
    connected: bool,
    cx: &App,
) -> AnyElement {
    Button::new(id)
        .ghost()
        .xsmall()
        .compact()
        .icon(IconName::Ellipsis)
        .hover_bg(workspace_row_highlight(cx))
        .tooltip("All windows")
        .disabled(!connected)
        .dropdown_menu(move |menu, _, _| {
            windows.iter().fold(menu, |menu, entry| {
                let select = entry.select.clone();
                menu.item(
                    PopupMenuItem::new(entry.label.clone())
                        .icon(if entry.active {
                            IconName::Check
                        } else {
                            IconName::AppWindow
                        })
                        .on_click(move |_, window, cx| select(window, cx)),
                )
            })
        })
        .into_any_element()
}

pub fn status_agent_count(id: impl Into<ElementId>, count: usize, cx: &App) -> AnyElement {
    let label = if count == 1 {
        "1 agent".to_owned()
    } else {
        format!("{count} agents")
    };
    workspace_status_item(id, Some(IconName::Bot), label.into(), cx).into_any_element()
}

pub fn status_clock(id: impl Into<ElementId>, label: SharedString, cx: &App) -> AnyElement {
    workspace_status_item(id, Some(IconName::Clock), label, cx).into_any_element()
}
