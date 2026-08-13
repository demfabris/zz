//! PARKED (not compiled): the touch-first chrome — status inset, top bar,
//! workspace, drawer, key strip. Revive it with `drawer.rs` and `settings.rs`.

use std::collections::BTreeSet;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, Global, InteractiveElement as _,
    IntoElement, KeyUpEvent, Keystroke, Modifiers, MouseButton, ParentElement as _, Render,
    ScrollHandle, Styled as _, WeakEntity, Window, div, px,
};
use zz::engine::{
    AppFpsMeter, IosAccessory,
    config::{self, settings::OpenSettings},
    mux::{HostId, MuxClient},
    nav::{MuxTreeModel, TreeNode, TreeTarget, expand_path_to, session_label},
    theme::chrome_background,
    ui_scale::scale_by,
    workspace::{AppView, WorkspaceRoute, WorkspaceSidebar},
};
use zz_gpui_ios::{keyboard_inset, take_content_size_scale, take_pinch_scale};
use zz_protocol::SessionId;
use zz_ui::{
    Root, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    navigation::{sidebar_settings_button, workspace_sidebar_divider},
    shell::app_shell_surface,
};

use crate::settings::IosSettings;

#[path = "drawer.rs"]
mod drawer;

const STATUS_BAR_INSET: f32 = 24.0;
const TOP_BAR_HEIGHT: f32 = 44.0;
const STRIP_HEIGHT: f32 = 40.0;

/// How the platform foreground hook finds the mux without owning it.
pub(crate) struct IosMuxHandle(pub WeakEntity<MuxClient>);

impl Global for IosMuxHandle {}

/// Retry hosts whose timers stopped while iOS suspended the process.
pub(crate) fn nudge_reconnects(cx: &mut App) {
    let Some(mux) = cx
        .try_global::<IosMuxHandle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    mux.update(cx, |mux, cx| mux.retry_stalled_hosts(cx));
}

/// The root view for the iPad window.
pub(crate) struct IosChrome {
    workspace: Entity<AppView>,
    mux: Entity<MuxClient>,
    drawer_open: bool,
    sidebar: Entity<WorkspaceSidebar>,
    settings: Option<Entity<IosSettings>>,
    fps_meter: Entity<AppFpsMeter>,
    drawer_scroll: ScrollHandle,
    expanded: BTreeSet<TreeNode>,
    last_attachment: Option<(HostId, Option<SessionId>)>,
    sidebar_focus_revision: u64,
}

impl IosChrome {
    pub(crate) fn new(
        workspace: Entity<AppView>,
        mux: Entity<MuxClient>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sidebar = workspace.read(cx).sidebar();
        let sidebar_focus_revision = mux.read(cx).sidebar_focus_revision();
        cx.observe(&mux, |chrome, mux, cx| {
            let revision = mux.read(cx).sidebar_focus_revision();
            if revision != chrome.sidebar_focus_revision {
                chrome.sidebar_focus_revision = revision;
                chrome.drawer_open = true;
            }
            cx.notify();
        })
        .detach();
        cx.observe(&sidebar, |_, _, cx| cx.notify()).detach();

        Self {
            workspace,
            mux,
            drawer_open: false,
            sidebar,
            settings: None,
            fps_meter: cx.new(|cx| AppFpsMeter::new(window, cx)),
            drawer_scroll: ScrollHandle::default(),
            expanded: BTreeSet::new(),
            last_attachment: None,
            sidebar_focus_revision,
        }
    }

    fn ensure_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_none() {
            let sidebar = self.sidebar.clone();
            self.settings = Some(cx.new(|cx| IosSettings::new(sidebar, window, cx)));
        }
    }

    fn enter_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_settings(window, cx);
        self.drawer_open = false;
        self.sidebar
            .update(cx, |sidebar, cx| sidebar.open_settings_route(cx));
        cx.notify();
    }

    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.sidebar.read(cx).route() {
            WorkspaceRoute::App => self.enter_settings(window, cx),
            WorkspaceRoute::Settings => {
                if let Some(settings) = self.settings.clone() {
                    settings.update(cx, |settings, cx| settings.close(window, cx));
                } else {
                    self.sidebar
                        .update(cx, |sidebar, cx| sidebar.close_settings(window, cx));
                }
            }
        }
    }

    fn attached_title(&self, cx: &App) -> String {
        let mux = self.mux.read(cx);
        if let Some(attached) = mux.attached_session()
            && let Some(session) = mux
                .snapshot()
                .sessions
                .iter()
                .find(|session| session.id == attached)
        {
            return session_label(&session.name, session.id);
        }

        let attached_host = mux.attached_host();
        mux.fleet_hosts()
            .find_map(|(host, name, _, _)| (host == attached_host).then(|| name.to_owned()))
            .unwrap_or_else(|| "zz".to_owned())
    }

    fn seed_attached_path(&mut self, cx: &App) {
        let mux = self.mux.read(cx);
        let attachment = (mux.attached_host(), mux.attached_session());
        if self.last_attachment == Some(attachment) {
            return;
        }

        let model = MuxTreeModel::from_mux(&mux);
        self.last_attachment = Some(attachment);
        if let Some(active) = model.active_target {
            expand_path_to(&mut self.expanded, &model, active);
        }
        self.expanded.insert(TreeNode::Host(attachment.0));
        if let Some(session) = attachment.1 {
            self.expanded
                .insert(TreeNode::Target(attachment.0, TreeTarget::Session(session)));
        }
    }

    fn render_top_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let chrome = cx.entity().clone();
        let drawer_button = Button::new("ios-drawer-toggle")
            .ghost()
            .xsmall()
            .compact()
            .w(px(TOP_BAR_HEIGHT))
            .h(px(TOP_BAR_HEIGHT))
            .icon(zz_ui::IconName::PanelLeft)
            .tooltip("Navigation")
            .on_click(move |_, _, cx| {
                chrome.update(cx, |chrome, cx| {
                    chrome.drawer_open = !chrome.drawer_open;
                    cx.notify();
                });
            });

        let route = self.sidebar.read(cx).route();
        let chrome = cx.entity().clone();
        let settings_button = sidebar_settings_button("ios-settings")
            .w(px(TOP_BAR_HEIGHT))
            .h(px(TOP_BAR_HEIGHT))
            .selected(route == WorkspaceRoute::Settings)
            .on_click(move |_, window, cx| {
                chrome.update(cx, |chrome, cx| chrome.toggle_settings(window, cx));
            });

        div()
            .h(px(TOP_BAR_HEIGHT))
            .w_full()
            .flex()
            .flex_none()
            .items_center()
            .bg(chrome_background(cx))
            .border_b_1()
            .border_color(workspace_sidebar_divider(cx))
            .child(drawer_button)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_center()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.attached_title(cx)),
            )
            .child(settings_button)
            .into_any_element()
    }

    fn strip_key(
        &self,
        label: &'static str,
        key: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        strip_button(label, false).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, window, cx| {
                let sticky = cx.try_global::<IosAccessory>().copied().unwrap_or_default();
                let keystroke = Keystroke {
                    modifiers: Modifiers {
                        control: sticky.ctrl,
                        alt: sticky.alt,
                        ..Modifiers::default()
                    },
                    key: key.to_owned(),
                    key_char: None,
                };
                // GPUI reentrancy rule: root listeners defer dispatch until the entity lease ends.
                window.defer(cx, move |window, cx| {
                    window.dispatch_keystroke(keystroke, cx);
                });
                cx.set_global(IosAccessory::default());
                cx.notify();
            }),
        )
    }

    fn strip_modifier(
        &self,
        label: &'static str,
        ctrl: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let sticky = cx.try_global::<IosAccessory>().copied().unwrap_or_default();
        let active = if ctrl { sticky.ctrl } else { sticky.alt };
        strip_button(label, active).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |_, _, _, cx| {
                let mut sticky = cx.try_global::<IosAccessory>().copied().unwrap_or_default();
                if ctrl {
                    sticky.ctrl = !sticky.ctrl;
                } else {
                    sticky.alt = !sticky.alt;
                }
                cx.set_global(sticky);
                cx.notify();
            }),
        )
    }

    fn render_key_strip(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .h(px(STRIP_HEIGHT))
            .flex_shrink_0()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .child(self.strip_key("esc", "escape", cx))
            .child(self.strip_key("tab", "tab", cx))
            .child(self.strip_modifier("ctrl", true, cx))
            .child(self.strip_modifier("alt", false, cx))
            .child(self.strip_key("←", "left", cx))
            .child(self.strip_key("↓", "down", cx))
            .child(self.strip_key("↑", "up", cx))
            .child(self.strip_key("→", "right", cx))
    }
}

impl Render for IosChrome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(scale) = take_pinch_scale() {
            scale_by(scale, cx);
        }
        if let Some(scale) = take_content_size_scale() {
            scale_by(scale, cx);
        }

        self.seed_attached_path(cx);
        let show_fps = config::show_fps(cx);
        self.fps_meter
            .update(cx, |meter, cx| meter.set_enabled(show_fps, cx));
        let route = self.sidebar.read(cx).route();
        if route == WorkspaceRoute::Settings {
            self.ensure_settings(window, cx);
        }

        let mut overlays = Vec::new();
        if show_fps {
            overlays.push(
                div()
                    .absolute()
                    .top(px(6.0))
                    .right(px(8.0))
                    .child(self.fps_meter.clone())
                    .into_any_element(),
            );
        }
        if self.drawer_open {
            overlays.push(self.render_drawer(window, cx));
        }
        overlays.extend(
            Root::render_dialog_layer(window, cx)
                .into_iter()
                .map(IntoElement::into_any_element),
        );
        overlays.extend(
            Root::render_notification_layer(window, cx)
                .into_iter()
                .map(IntoElement::into_any_element),
        );

        let content = match route {
            WorkspaceRoute::App => {
                app_shell_surface("ios-shell", div(), None, self.workspace.clone(), overlays)
                    .into_any_element()
            }
            WorkspaceRoute::Settings => div()
                .relative()
                .size_full()
                .child(
                    self.settings
                        .clone()
                        .expect("settings is created before the route renders"),
                )
                .children(overlays)
                .into_any_element(),
        };
        let root = div()
            .id("ios-chrome")
            .size_full()
            .flex()
            .flex_col()
            .bg(chrome_background(cx))
            .pt(px(STATUS_BAR_INSET))
            .pb(px(keyboard_inset()))
            .child(self.render_top_bar(cx))
            .child(div().flex_1().min_h(px(0.0)).child(content))
            .child(self.render_key_strip(cx))
            .capture_key_up(cx.listener(|chrome, event: &KeyUpEvent, window, cx| {
                chrome.workspace.update(cx, |workspace, cx| {
                    workspace.on_claim_key_up(event, window, cx);
                });
            }));

        let chrome = cx.entity().clone();
        root.on_action(move |_: &OpenSettings, window, cx| {
            chrome.update(cx, |chrome, cx| chrome.toggle_settings(window, cx));
        })
    }
}

fn strip_button(label: &'static str, active: bool) -> gpui::Div {
    let overlay = if active { 0.22 } else { 0.08 };
    div()
        .px(px(14.0))
        .py(px(5.0))
        .rounded(px(8.0))
        .bg(gpui::white().opacity(overlay))
        .text_size(px(13.0))
        .child(label)
}
