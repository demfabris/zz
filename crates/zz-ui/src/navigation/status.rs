use std::{ops::Range, rc::Rc};

mod pane_deck;
pub use pane_deck::{StatusPaneEntry, status_pane_deck};
pub use zz_client::AgentAttentionStatus;

use gpui::{
    AnyElement, App, ElementId, IntoElement, MouseButton, SharedString, Window, div, prelude::*, px,
};

use crate::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Sizable as _,
    StyledExt as _,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
};

use super::{
    WorkspaceStatusWindowState, workspace_row_highlight, workspace_status_window,
    workspace_tree_action_button,
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

pub struct StatusSessionEntry {
    pub label: SharedString,
    pub active: bool,
    pub select: StatusAction,
}

pub struct StatusAgentEntry {
    pub label: SharedString,
    pub window_name: SharedString,
    pub icon: IconName,
    pub status: Option<zz_client::AgentAttentionStatus>,
    pub select: StatusAction,
}

pub fn visible_window_range(total: usize, active: usize, limit: usize) -> Range<usize> {
    if total <= limit || limit == 0 {
        return 0..total;
    }
    let start = active.saturating_sub(limit / 2).min(total - limit);
    start..start + limit
}

fn status_menu_button(id: impl Into<ElementId>, cx: &App) -> Button {
    Button::new(id)
        .ghost()
        .flat()
        .xsmall()
        .compact()
        .h(px(30.0))
        .bg(workspace_row_highlight(cx))
        .hover_bg(workspace_row_highlight(cx))
        .when(cx.theme().shadow, |button| {
            button.border(px(0.5)).control_highlight(cx)
        })
}

pub fn status_session(
    id: impl Into<ElementId>,
    name: SharedString,
    sessions: Vec<StatusSessionEntry>,
    connected: bool,
    cx: &App,
) -> AnyElement {
    status_menu_button(id, cx)
        .flex_none()
        .max_w(px(180.0))
        .px(px(7.0))
        .icon(Icon::new(IconName::Layers).size(px(13.0)).top(px(0.5)))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(name),
        )
        .child(Icon::new(IconName::ChevronDown).size(px(11.0)))
        .tooltip("Switch session")
        .disabled(!connected)
        .dropdown_menu(move |menu, _, _| {
            sessions.iter().fold(menu.label("Sessions"), |menu, entry| {
                let select = entry.select.clone();
                menu.item(
                    PopupMenuItem::new(entry.label.clone())
                        .icon(if entry.active {
                            IconName::Check
                        } else {
                            IconName::Layers
                        })
                        .checked(entry.active)
                        .on_click(move |_, window, cx| select(window, cx)),
                )
            })
        })
        .into_any_element()
}

pub fn status_window(
    id: impl Into<ElementId>,
    index: SharedString,
    name: SharedString,
    state: WorkspaceStatusWindowState,
    panes: Vec<StatusPaneEntry>,
    actions: StatusWindowActions,
    cx: &App,
) -> AnyElement {
    let id = id.into();
    let group: SharedString = format!("status-window-{id:?}").into();
    let close_button = actions.close.clone();
    let tooltip = format!("{index}:{name}").into();
    let deck = status_pane_deck(
        ElementId::Name(format!("status-window-panes-{id:?}").into()),
        panes,
        state.connected,
        cx,
    );
    let item = workspace_status_window(id.clone(), index, name, tooltip, state, deck, cx)
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

fn agent_status_label(status: Option<zz_client::AgentAttentionStatus>) -> &'static str {
    use zz_client::AgentAttentionStatus;
    match status {
        Some(AgentAttentionStatus::Working) => "Running",
        Some(AgentAttentionStatus::NeedsInput) => "Needs input",
        Some(AgentAttentionStatus::Failed) => "Failed",
        Some(AgentAttentionStatus::Idle) => "Idle",
        None => "Connecting",
    }
}

fn agent_summary(agents: &[StatusAgentEntry]) -> (String, Option<zz_client::AgentAttentionStatus>) {
    use zz_client::AgentAttentionStatus;
    for (status, label) in [
        (AgentAttentionStatus::NeedsInput, "needs input"),
        (AgentAttentionStatus::Failed, "failed"),
        (AgentAttentionStatus::Working, "running"),
    ] {
        let count = agents
            .iter()
            .filter(|agent| agent.status == Some(status))
            .count();
        if count > 0 {
            return (format!("{count} {label}"), Some(status));
        }
    }
    if agents
        .iter()
        .all(|agent| agent.status == Some(AgentAttentionStatus::Idle))
    {
        (
            format!("{} idle", agents.len()),
            Some(AgentAttentionStatus::Idle),
        )
    } else {
        (
            format!(
                "{} agent{}",
                agents.len(),
                if agents.len() == 1 { "" } else { "s" }
            ),
            None,
        )
    }
}

pub fn status_agents(
    id: impl Into<ElementId>,
    agents: Vec<StatusAgentEntry>,
    connected: bool,
    cx: &App,
) -> AnyElement {
    use zz_client::AgentAttentionStatus;
    let (label, status) = agent_summary(&agents);
    let color = match status {
        Some(AgentAttentionStatus::NeedsInput) => cx.theme().warning,
        Some(AgentAttentionStatus::Failed) => cx.theme().danger,
        Some(AgentAttentionStatus::Working) => cx.theme().success,
        _ => cx.theme().foreground.muted(),
    };
    status_menu_button(id, cx)
        .px(px(6.0))
        .text_color(cx.theme().foreground.muted())
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(div().flex_none().size(px(5.0)).rounded_full().bg(color))
                .child(label),
        )
        .tooltip("Agent activity")
        .disabled(!connected)
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
            agents
                .iter()
                .fold(menu.label("Agent activity"), |menu, entry| {
                    let select = entry.select.clone();
                    menu.item(
                        PopupMenuItem::new(format!(
                            "{} · {} · {}",
                            entry.label,
                            entry.window_name,
                            agent_status_label(entry.status)
                        ))
                        .icon(entry.icon.clone())
                        .on_click(move |_, window, cx| select(window, cx)),
                    )
                })
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Modifiers, Render, TestAppContext, VisualTestContext};
    use std::cell::Cell;

    fn agent(status: Option<AgentAttentionStatus>) -> StatusAgentEntry {
        StatusAgentEntry {
            label: "Review titlebar".into(),
            window_name: "workspace".into(),
            icon: IconName::Bot,
            status,
            select: Rc::new(|_, _| {}),
        }
    }

    #[test]
    fn activity_prioritizes_attention_and_never_counts_unknown_state_as_running() {
        use AgentAttentionStatus::{Failed, Idle, NeedsInput, Working};
        let mut agents = vec![agent(Some(Working)), agent(Some(Idle)), agent(None)];
        assert_eq!(agent_summary(&agents), ("1 running".into(), Some(Working)));
        agents.push(agent(Some(Failed)));
        assert_eq!(agent_summary(&agents), ("1 failed".into(), Some(Failed)));
        agents.push(agent(Some(NeedsInput)));
        assert_eq!(
            agent_summary(&agents),
            ("1 needs input".into(), Some(NeedsInput))
        );
        assert_eq!(
            agent_summary(&[agent(Some(Idle))]),
            ("1 idle".into(), Some(Idle))
        );
        assert_eq!(agent_summary(&[agent(None)]), ("1 agent".into(), None));
    }

    struct StatusTest {
        selected: Rc<Cell<usize>>,
        connected: bool,
    }

    impl Render for StatusTest {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let session_selected = self.selected.clone();
            let agent_selected = self.selected.clone();
            let mut entry = agent(Some(AgentAttentionStatus::Working));
            entry.select = Rc::new(move |_, _| agent_selected.set(2));
            div()
                .flex()
                .gap(px(12.0))
                .child(
                    div()
                        .h(px(30.0))
                        .debug_selector(|| "session-trigger".into())
                        .child(status_session(
                            "test-session",
                            "current".into(),
                            vec![StatusSessionEntry {
                                label: "other".into(),
                                active: false,
                                select: Rc::new(move |_, _| session_selected.set(1)),
                            }],
                            self.connected,
                            cx,
                        )),
                )
                .child(
                    div()
                        .h(px(30.0))
                        .debug_selector(|| "agents-trigger".into())
                        .child(status_agents(
                            "test-agents",
                            vec![entry],
                            self.connected,
                            cx,
                        )),
                )
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| {
            _ = window.draw(cx);
        });
    }

    #[gpui::test]
    fn menus_select_their_targets_and_disable_when_disconnected(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let selected = Rc::new(Cell::new(0));
        let (view, cx) = cx.add_window_view({
            let selected = selected.clone();
            move |_, _| StatusTest {
                selected,
                connected: true,
            }
        });
        for (selector, target) in [("session-trigger", 1), ("agents-trigger", 2)] {
            draw(cx);
            let center = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_click(center, Modifiers::none());
            draw(cx);
            cx.update(|window, cx| assert!(window.focused(cx).is_some()));
            cx.simulate_keystrokes("down down enter");
            draw(cx);
            assert_eq!(selected.get(), target);
        }
        selected.set(0);
        view.update(cx, |view, cx| {
            view.connected = false;
            cx.notify();
        });
        for selector in ["session-trigger", "agents-trigger"] {
            draw(cx);
            let center = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_click(center, Modifiers::none());
            draw(cx);
            cx.simulate_keystrokes("down down enter");
            draw(cx);
            assert_eq!(selected.get(), 0);
        }
    }
}
