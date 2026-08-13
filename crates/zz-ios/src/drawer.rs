//! PARKED (not compiled): touch-first drawer — see touch_chrome.rs header.
use gpui::{
    AnyElement, App, Context, ElementId, Entity, FontWeight, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};
use zz::engine::{
    nav::{
        HostIndicator, MuxTreeHost, MuxTreeModel, MuxTreeSession, MuxTreeWindow, NavActivation,
        TreeNode, activate_nav, session_initial,
    },
    theme::chrome_background,
};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _,
    navigation::{workspace_row_highlight, workspace_sidebar_divider},
    scroll::ScrollableElement as _,
    spinner::Spinner,
};

use super::IosChrome;

const DRAWER_MAX_WIDTH: f32 = 320.0;
const HOST_ROW_HEIGHT: f32 = 44.0;
const SESSION_ROW_HEIGHT: f32 = 44.0;
const WINDOW_ROW_HEIGHT: f32 = 40.0;
const FOOTER_ROW_HEIGHT: f32 = 44.0;
const HOST_INDENT: f32 = 10.0;
const SESSION_INDENT: f32 = 32.0;
const WINDOW_INDENT: f32 = 64.0;

impl IosChrome {
    pub(super) fn render_drawer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = px(DRAWER_MAX_WIDTH).min(window.viewport_size().width * 0.85);
        let body = self.render_touch_navigation(cx);

        let dismiss = cx.entity().clone();
        let scrim = div()
            .id("ios-drawer-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_start()
            .bg(cx.theme().scrim)
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                dismiss.update(cx, |chrome, cx| {
                    chrome.drawer_open = false;
                    cx.notify();
                });
                cx.stop_propagation();
            });

        scrim
            .child(
                div()
                    .id("ios-drawer-panel")
                    .h_full()
                    .w(width)
                    .flex()
                    .flex_none()
                    .flex_col()
                    .overflow_hidden()
                    .bg(chrome_background(cx))
                    .border_r_1()
                    .border_color(workspace_sidebar_divider(cx))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(body),
            )
            .into_any_element()
    }

    fn render_touch_navigation(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (model, attached_host, attached_session) = {
            let mux = self.mux.read(cx);
            (
                MuxTreeModel::from_mux(&mux),
                mux.attached_host(),
                mux.attached_session(),
            )
        };
        let chrome = cx.entity().clone();
        let mut rows = Vec::new();

        for host in &model.hosts {
            let host_node = host.node();
            let host_expanded = self.expanded.contains(&host_node);
            let activation = model.activation_for_node(host_node, attached_host, attached_session);
            let indicator = host.indicator();
            let failure_detail = match &indicator {
                Some(HostIndicator::Failed { detail }) => detail.clone(),
                Some(HostIndicator::Connecting) | None => None,
            };
            rows.push(Self::host_row(
                host,
                host_expanded,
                host.id == attached_host,
                indicator,
                activation,
                chrome.clone(),
                cx,
            ));
            if let Some(detail) = failure_detail {
                rows.push(Self::failure_detail(host.id, detail, cx));
            }

            if !host_expanded {
                continue;
            }
            for session in &host.sessions {
                let session_node = TreeNode::Target(host.id, session.target());
                let session_attached =
                    host.id == attached_host && Some(session.id) == attached_session;
                let session_expanded = self.expanded.contains(&session_node);
                let activation = (!session_attached)
                    .then(|| {
                        model.activation_for_node(session_node, attached_host, attached_session)
                    })
                    .flatten();
                rows.push(Self::session_row(
                    host.id,
                    session,
                    session_expanded,
                    session_attached,
                    activation,
                    chrome.clone(),
                    cx,
                ));

                if session_expanded {
                    for mux_window in &session.windows {
                        let window_node = TreeNode::Target(host.id, mux_window.target());
                        let activation =
                            model.activation_for_node(window_node, attached_host, attached_session);
                        rows.push(Self::window_row(
                            host.id,
                            mux_window,
                            activation,
                            chrome.clone(),
                            cx,
                        ));
                    }
                }
            }

            if host.connected() {
                rows.push(Self::new_session_row(host.id, &self.mux, cx));
            }
        }

        let scroll = self.drawer_scroll.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .relative()
                    .child(
                        div()
                            .id("ios-drawer-scroll")
                            .size_full()
                            .flex()
                            .flex_col()
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .py(px(6.0))
                            .children(rows),
                    )
                    .vertical_scrollbar(&scroll),
            )
            .child(self.render_drawer_footer(cx))
            .into_any_element()
    }

    fn host_row(
        host: &MuxTreeHost,
        expanded: bool,
        attached: bool,
        indicator: Option<HostIndicator>,
        activation: Option<NavActivation>,
        chrome: Entity<Self>,
        cx: &App,
    ) -> AnyElement {
        let node = host.node();
        let status = match indicator {
            None => div().into_any_element(),
            Some(HostIndicator::Connecting) => Spinner::new()
                .xsmall()
                .color(cx.theme().foreground.muted())
                .into_any_element(),
            Some(HostIndicator::Failed { .. }) => Icon::new(IconName::TriangleAlert)
                .xsmall()
                .text_color(cx.theme().warning)
                .into_any_element(),
        };
        let label = div()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .font_weight(FontWeight::SEMIBOLD)
            .child(host.name.clone())
            .into_any_element();
        let trailing = Self::disclosure(expanded, cx);
        let can_toggle = attached;

        Self::drawer_row(
            format!("ios-drawer-host-{}", node.tree_id()),
            HOST_ROW_HEIGHT,
            HOST_INDENT,
            false,
            status,
            label,
            Some(trailing),
            cx,
        )
        .on_click(move |_, _, cx| {
            if let Some(activation) = activation.clone() {
                chrome.update(cx, |chrome, cx| {
                    chrome.activate_from_drawer(activation, cx);
                });
            } else if can_toggle {
                chrome.update(cx, |chrome, cx| chrome.toggle_drawer_node(node, cx));
            }
            cx.stop_propagation();
        })
        .into_any_element()
    }

    fn failure_detail(host: zz::engine::mux::HostId, detail: SharedString, cx: &App) -> AnyElement {
        div()
            .id(format!("ios-drawer-host-failure-{host:?}"))
            .w_full()
            .pl(px(44.0))
            .pr(px(12.0))
            .pb(px(8.0))
            .whitespace_normal()
            .text_size(px(12.0))
            .text_color(cx.theme().foreground.muted())
            .child(detail)
            .into_any_element()
    }

    fn session_row(
        host: zz::engine::mux::HostId,
        session: &MuxTreeSession,
        expanded: bool,
        attached: bool,
        activation: Option<NavActivation>,
        chrome: Entity<Self>,
        cx: &App,
    ) -> AnyElement {
        let node = TreeNode::Target(host, session.target());
        let label = session.label();
        let badge = div()
            .size(px(24.0))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(workspace_row_highlight(cx))
            .text_size(px(12.0))
            .font_weight(FontWeight::SEMIBOLD)
            .child(session_initial(&label))
            .into_any_element();
        let label = div()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(label)
            .into_any_element();
        let trailing = (!session.windows.is_empty()).then(|| Self::disclosure(expanded, cx));

        Self::drawer_row(
            format!("ios-drawer-session-{}", node.tree_id()),
            SESSION_ROW_HEIGHT,
            SESSION_INDENT,
            session.active,
            badge,
            label,
            trailing,
            cx,
        )
        .on_click(move |_, _, cx| {
            if attached {
                chrome.update(cx, |chrome, cx| chrome.toggle_drawer_node(node, cx));
            } else if let Some(activation) = activation.clone() {
                chrome.update(cx, |chrome, cx| {
                    chrome.activate_from_drawer(activation, cx);
                });
            }
            cx.stop_propagation();
        })
        .into_any_element()
    }

    fn window_row(
        host: zz::engine::mux::HostId,
        mux_window: &MuxTreeWindow,
        activation: Option<NavActivation>,
        chrome: Entity<Self>,
        cx: &App,
    ) -> AnyElement {
        let node = TreeNode::Target(host, mux_window.target());
        let leading = Icon::new(IconName::GalleryVerticalEnd)
            .xsmall()
            .text_color(cx.theme().foreground.muted())
            .into_any_element();
        let label = div()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(mux_window.name.clone())
            .into_any_element();
        let trailing = mux_window.active.then(|| {
            Icon::new(IconName::Check)
                .xsmall()
                .text_color(cx.theme().foreground)
                .into_any_element()
        });

        Self::drawer_row(
            format!("ios-drawer-window-{}", node.tree_id()),
            WINDOW_ROW_HEIGHT,
            WINDOW_INDENT,
            false,
            leading,
            label,
            trailing,
            cx,
        )
        .on_click(move |_, _, cx| {
            if let Some(activation) = activation.clone() {
                chrome.update(cx, |chrome, cx| {
                    chrome.activate_from_drawer(activation, cx);
                });
            }
            cx.stop_propagation();
        })
        .into_any_element()
    }

    fn new_session_row(
        host: zz::engine::mux::HostId,
        mux: &Entity<zz::engine::mux::MuxClient>,
        cx: &App,
    ) -> AnyElement {
        let new_session_mux = mux.clone();
        let leading = Icon::new(IconName::Plus)
            .xsmall()
            .text_color(cx.theme().foreground.muted())
            .into_any_element();
        let label = div()
            .text_color(cx.theme().foreground.muted())
            .child("New session")
            .into_any_element();
        Self::drawer_row(
            format!("ios-drawer-new-session-{host:?}"),
            WINDOW_ROW_HEIGHT,
            SESSION_INDENT,
            false,
            leading,
            label,
            None,
            cx,
        )
        .on_click(move |_, _, cx| {
            new_session_mux.read(cx).new_session(host);
            cx.stop_propagation();
        })
        .into_any_element()
    }

    fn render_drawer_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let close_after_add = cx.entity().clone();
        let add_host = Self::footer_row("ios-drawer-add-host", IconName::Plus, "Add host…", cx)
            .on_click(move |_, window, cx| {
                close_after_add.update(cx, |chrome, cx| {
                    chrome.drawer_open = false;
                    cx.notify();
                });
                zz::engine::workspace::add_host::open(window, cx);
                cx.stop_propagation();
            });

        let close_after_settings = cx.entity().clone();
        let settings = Self::footer_row("ios-drawer-settings", IconName::Settings, "Settings", cx)
            .on_click(move |_, window, cx| {
                close_after_settings.update(cx, |chrome, cx| {
                    chrome.enter_settings(window, cx);
                });
                cx.stop_propagation();
            });

        div()
            .flex()
            .flex_none()
            .flex_col()
            .border_t_1()
            .border_color(workspace_sidebar_divider(cx))
            .py(px(4.0))
            .child(add_host)
            .child(settings)
            .into_any_element()
    }

    fn footer_row(
        id: &'static str,
        icon: IconName,
        label: &'static str,
        cx: &App,
    ) -> Stateful<gpui::Div> {
        Self::drawer_row(
            id,
            FOOTER_ROW_HEIGHT,
            HOST_INDENT,
            false,
            Icon::new(icon).xsmall().into_any_element(),
            div().child(label).into_any_element(),
            None,
            cx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn drawer_row(
        id: impl Into<ElementId>,
        height: f32,
        indent: f32,
        active: bool,
        leading: AnyElement,
        label: AnyElement,
        trailing: Option<AnyElement>,
        cx: &App,
    ) -> Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(height))
            .w_full()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .pl(px(indent))
            .pr(px(10.0))
            .rounded(px(8.0))
            .cursor_pointer()
            .when(active, |row| row.bg(workspace_row_highlight(cx)))
            .child(
                div()
                    .w(px(24.0))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .child(leading),
            )
            .child(div().flex_1().min_w_0().child(label))
            .children(trailing)
    }

    fn disclosure(expanded: bool, cx: &App) -> AnyElement {
        Icon::new(if expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        })
        .xsmall()
        .text_color(cx.theme().foreground.muted())
        .into_any_element()
    }

    fn toggle_drawer_node(&mut self, node: TreeNode, cx: &mut Context<Self>) {
        if !self.expanded.remove(&node) {
            self.expanded.insert(node);
        }
        cx.notify();
    }

    fn activate_from_drawer(&mut self, activation: NavActivation, cx: &mut Context<Self>) {
        activate_nav(&self.mux, activation, cx);
        self.drawer_open = false;
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.release_focus(cx));
        cx.notify();
    }
}
