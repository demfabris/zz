use std::{collections::VecDeque, rc::Rc};

use futures::StreamExt as _;
use gpui::{
    App, ClipboardEntry, ClipboardItem, ClipboardString, Context, Corners, Entity, FocusHandle,
    Focusable, Image, ImageFormat, Pixels, Render, Subscription, Window, div, prelude::*, px,
};
use zz_gpui_ios::browser::{BrowserEvent, BrowserProfile, BrowserView, ElementPickerAppearance};
use zz_protocol::{BrowserCommand, BrowserDescriptor, PaneId};
use zz_ui::{
    ActiveTheme as _, Colorize as _, ElementExt as _, Icon, IconName, Sizable as _,
    browser::{
        BrowserHeader, BrowserTabInfo, BrowserTabStrip, BrowserToolbar, browser_address,
        browser_toolbar_button,
    },
    button::{Button, ButtonVariants as _},
    input::{InputEvent, InputState},
    pane::pane_header_icon_button,
};

use crate::connection::Connection;

struct Tab {
    id: u64,
    view: Rc<BrowserView>,
    url: String,
    title: String,
    loading: bool,
    back: bool,
    forward: bool,
    pending_load: bool,
    navigating: bool,
    error: Option<String>,
    picking: bool,
    pick_status: Option<String>,
    _events: gpui::Task<()>,
}

pub(super) struct BrowserPane {
    pane: PaneId,
    host: String,
    profile_name: String,
    profile: Rc<BrowserProfile>,
    connection: Entity<Connection>,
    address: Entity<InputState>,
    focus: FocusHandle,
    tabs: Vec<Tab>,
    active: usize,
    next_tab: u64,
    received: BrowserDescriptor,
    pending: VecDeque<BrowserDescriptor>,
    visible: bool,
    active_pane: bool,
    radii: Corners<Pixels>,
    _subscriptions: Vec<Subscription>,
}

impl BrowserPane {
    pub(super) fn new(
        pane: PaneId,
        descriptor: &BrowserDescriptor,
        connection: Entity<Connection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let host = connection
            .read(cx)
            .endpoint()
            .unwrap_or_default()
            .to_owned();
        let profile = BrowserProfile::new(&host, &descriptor.profile);
        profile.set_client(connection.read(cx).browser_client());
        let address =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or enter address"));
        let submit = cx.subscribe_in(
            &address,
            window,
            |this: &mut Self, input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let value = input.read(cx).value().to_string();
                    this.navigate(&value, window, cx);
                }
            },
        );
        let commands = cx.subscribe_in(
            &connection,
            window,
            |this: &mut Self, _, event: &zz_client::CoreEvent, window, cx| {
                if let zz_client::CoreEvent::BrowserCommand { pane, command } = event {
                    if *pane != this.pane || !this.writable(cx) {
                        return;
                    }
                    match command {
                        BrowserCommand::Navigate(url) => this.navigate(url, window, cx),
                        BrowserCommand::Reload => this.reload(cx),
                        BrowserCommand::Back => this.tabs[this.active].view.back(),
                        BrowserCommand::Forward => this.tabs[this.active].view.forward(),
                        _ => {}
                    }
                }
            },
        );
        let descriptor = normalized(descriptor);
        let mut this = Self {
            pane,
            host,
            profile_name: descriptor.profile.clone(),
            profile,
            connection,
            address,
            focus: cx.focus_handle(),
            tabs: Vec::new(),
            active: descriptor.active_tab,
            next_tab: 0,
            received: descriptor.clone(),
            pending: VecDeque::new(),
            visible: false,
            active_pane: false,
            radii: Corners::default(),
            _subscriptions: vec![submit, commands],
        };
        for url in &descriptor.tabs {
            let tab = this.new_tab(url.clone(), window, cx);
            this.tabs.push(tab);
        }
        this.update_address(window, cx);
        this.resume();
        this
    }

    pub(super) fn matches(&self, host: &str, profile: &str) -> bool {
        self.host == host && self.profile_name == profile
    }

    pub(super) fn belongs_to_host(&self, host: &str) -> bool {
        self.host == host
    }

    fn writable(&self, cx: &App) -> bool {
        self.connection.read(cx).connected && !self.connection.read(cx).core.attached_read_only()
    }

    fn toggle_element_pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.writable(cx) || !self.visible {
            return;
        }
        let tab = &mut self.tabs[self.active];
        tab.pick_status = None;
        if tab.picking {
            tab.view.cancel_element_pick();
            tab.picking = false;
        } else if !tab.loading {
            let theme = cx.theme();
            let appearance = ElementPickerAppearance {
                highlight_outline: zz_ui::to_hex(theme.foreground),
                highlight_fill: zz_ui::to_hex(theme.foreground.fill()),
                highlight_contrast: zz_ui::to_hex(theme.background.opaque()),
                preview_background: zz_ui::to_hex(theme.background.raised(1).opaque()),
                preview_foreground: zz_ui::to_hex(theme.foreground),
                preview_border: zz_ui::to_hex(theme.border()),
                shadow: theme.shadow.then(|| zz_ui::to_hex(theme.scrim)),
                radius: f32::from(theme.radius),
                font_family: theme.mono_font_family.to_string(),
                page_zoom: 1.0,
            };
            match tab.view.start_element_pick(&appearance) {
                Ok(()) => tab.picking = true,
                Err(error) => tab.pick_status = Some(error),
            }
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn new_tab(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) -> Tab {
        let id = self.next_tab;
        self.next_tab += 1;
        let (view, mut events) = BrowserView::new(self.profile.clone());
        let task = cx.spawn_in(window, async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update_in(cx, |this, window, cx| this.event(id, event, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Tab {
            id,
            view,
            url,
            title: String::new(),
            loading: false,
            back: false,
            forward: false,
            pending_load: true,
            navigating: false,
            error: None,
            picking: false,
            pick_status: None,
            _events: task,
        }
    }

    fn event(&mut self, id: u64, event: BrowserEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        match event {
            BrowserEvent::Changed {
                url,
                title,
                loading,
                can_go_back,
                can_go_forward,
                committed,
            } => {
                let tab = &mut self.tabs[index];
                if committed {
                    tab.navigating = false;
                    tab.error = None;
                }
                if !tab.pending_load
                    && !tab.navigating
                    && let Some(url) = url
                {
                    tab.url = url;
                }
                tab.title = title;
                tab.loading = loading;
                tab.back = can_go_back;
                tab.forward = can_go_forward;
                self.update_address(window, cx);
                self.publish(cx);
            }
            BrowserEvent::Error(error) => self.tabs[index].error = Some(error),
            BrowserEvent::ElementPicked { text, screenshot } => {
                self.tabs[index].picking = false;
                if index != self.active || !self.visible || !self.writable(cx) {
                    return;
                }
                let mut entries = vec![ClipboardEntry::String(ClipboardString::new(
                    text.to_string(),
                ))];
                let status = if let Some(png) = screenshot {
                    entries.push(ClipboardEntry::Image(Image::from_bytes(
                        ImageFormat::Png,
                        png,
                    )));
                    "Element context + screenshot copied"
                } else {
                    "Element context copied"
                };
                cx.write_to_clipboard(ClipboardItem { entries });
                self.tabs[index].pick_status = Some(status.into());
            }
            BrowserEvent::ElementPickCancelled => self.tabs[index].picking = false,
            BrowserEvent::ElementPickFailed => {
                self.tabs[index].picking = false;
                self.tabs[index].pick_status =
                    Some("Could not inspect that element. Try again.".into());
            }
            BrowserEvent::Open(url) => {
                if self.writable(cx) {
                    self.add_tab(url, window, cx);
                }
            }
            BrowserEvent::Focused => {
                self.focus.focus(window, cx);
                self.connection.update(cx, |connection, cx| {
                    connection.command(
                        "select-pane",
                        vec!["-Z".into(), "-t".into(), self.pane.to_string()],
                        cx,
                    )
                });
            }
        }
        cx.notify();
    }

    pub(super) fn synchronize(
        &mut self,
        descriptor: &BrowserDescriptor,
        active: bool,
        radii: Corners<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.active_pane != active
            || self.radii != radii
            || normalized(descriptor) != self.received;
        self.active_pane = active;
        self.radii = radii;
        let descriptor = normalized(descriptor);
        if descriptor != self.received {
            self.received = descriptor.clone();
            if let Some(index) = self
                .pending
                .iter()
                .position(|pending| *pending == descriptor)
            {
                self.pending.drain(..=index);
            } else {
                self.pending.clear();
                for (index, url) in descriptor.tabs.iter().enumerate() {
                    if let Some(tab) = self.tabs.get_mut(index) {
                        if tab.url != *url {
                            tab.url = url.clone();
                            tab.pending_load = true;
                            tab.view.stop();
                        }
                    } else {
                        let tab = self.new_tab(url.clone(), window, cx);
                        self.tabs.push(tab);
                    }
                }
                self.tabs.truncate(descriptor.tabs.len());
                self.active = descriptor.active_tab;
            }
            self.update_address(window, cx);
        }
        self.resume();
        if changed {
            cx.notify();
        }
    }

    pub(super) fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        let changed_route = self
            .profile
            .set_client(self.connection.read(cx).browser_client());
        let available = self.profile.available();
        if changed_route {
            for tab in &mut self.tabs {
                tab.view.stop();
                tab.pending_load = true;
            }
            if available {
                self.resume();
            }
        }
        let writable = self.writable(cx);
        for (index, tab) in self.tabs.iter().enumerate() {
            tab.view
                .set_visible(visible && available && index == self.active);
            tab.view.set_interactive(writable);
        }
        if self.visible != visible || changed_route {
            self.visible = visible;
            cx.notify();
        }
    }

    fn resume(&mut self) {
        if !self.profile.available() {
            return;
        }
        let tab = &mut self.tabs[self.active];
        if tab.pending_load {
            tab.navigating = true;
            tab.error = tab.view.load(&tab.url).err();
            if tab.error.is_some() {
                tab.navigating = false;
            }
            tab.pending_load = false;
        }
    }

    fn update_address(&self, window: &mut Window, cx: &mut Context<Self>) {
        if self.address.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let url = &self.tabs[self.active].url;
        let value = if url == "about:blank" { "" } else { url };
        if self.address.read(cx).value().as_str() != value {
            self.address
                .update(cx, |input, cx| input.set_value(value, window, cx));
        }
    }

    fn navigate(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.writable(cx) {
            return;
        }
        let Ok(url) = zz_client::url_input::resolve_address(
            value,
            zz_client::url_input::SearchProvider::default(),
        ) else {
            return;
        };
        let tab = &mut self.tabs[self.active];
        tab.url = url;
        tab.pick_status = None;
        tab.pending_load = true;
        tab.error = None;
        self.focus.focus(window, cx);
        self.resume();
        self.update_address(window, cx);
        self.publish(cx);
        cx.notify();
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        if !self.writable(cx) {
            return;
        }
        let tab = &mut self.tabs[self.active];
        if tab.error.take().is_some() {
            tab.pending_load = true;
        }
        if tab.loading {
            tab.view.stop();
        } else if tab.pending_load {
            self.resume();
        } else {
            tab.view.reload();
        }
        cx.notify();
    }

    fn select(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.writable(cx) {
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs[self.active].view.set_visible(false);
        self.active = index;
        self.focus.focus(window, cx);
        self.resume();
        self.update_address(window, cx);
        self.publish(cx);
        cx.notify();
    }

    fn add_tab(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.writable(cx) {
            return;
        }
        self.tabs[self.active].view.set_visible(false);
        let blank = url == "about:blank";
        let tab = self.new_tab(url, window, cx);
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.focus.focus(window, cx);
        self.update_address(window, cx);
        if blank {
            self.address.read(cx).focus_handle(cx).focus(window, cx);
        }
        self.resume();
        self.publish(cx);
        cx.notify();
    }

    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if !self.writable(cx) || self.tabs.len() == 1 {
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
        self.focus.focus(window, cx);
        self.resume();
        self.update_address(window, cx);
        self.publish(cx);
        cx.notify();
    }

    fn publish(&mut self, cx: &mut Context<Self>) {
        if !self.writable(cx) {
            return;
        }
        let descriptor = BrowserDescriptor {
            tabs: self.tabs.iter().map(|tab| tab.url.clone()).collect(),
            active_tab: self.active,
            profile: self.profile_name.clone(),
        };
        if &descriptor == self.pending.back().unwrap_or(&self.received) {
            return;
        }
        self.pending.push_back(descriptor.clone());
        let mut args = vec![
            "-t".into(),
            self.pane.to_string(),
            "-a".into(),
            self.active.to_string(),
            "--".into(),
        ];
        args.extend(descriptor.tabs);
        self.connection.update(cx, |connection, cx| {
            connection.command("set-browser-tabs", args, cx)
        });
    }
}

impl Focusable for BrowserPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BrowserPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let writable = self.writable(cx);
        let tab = &self.tabs[self.active];
        let tabs = self
            .tabs
            .iter()
            .map(|tab| {
                BrowserTabInfo::new(
                    tab.id,
                    if tab.title.is_empty() {
                        if tab.url == "about:blank" {
                            "New tab"
                        } else {
                            &tab.url
                        }
                    } else {
                        &tab.title
                    }
                    .to_owned(),
                    tab.url.clone(),
                )
            })
            .collect();
        let weak = cx.weak_entity();
        let close = weak.clone();
        let new = weak.clone();
        let strip = BrowserTabStrip::new(tabs, self.active)
            .on_activate(move |id, window, cx| {
                let _ = weak.update(cx, |this, cx| this.select(id, window, cx));
            })
            .on_close(move |id, window, cx| {
                let _ = close.update(cx, |this, cx| this.close_tab(id, window, cx));
            })
            .on_new_tab(move |window, cx| {
                let _ = new.update(cx, |this, cx| {
                    this.add_tab("about:blank".into(), window, cx)
                });
            });
        let toolbar = BrowserToolbar::new(
            browser_toolbar_button(
                cx,
                "ios-browser-back",
                IconName::ArrowLeft,
                "Back",
                !writable || !tab.back,
                false,
            )
            .on_click(cx.listener(|this, _, _, _| this.tabs[this.active].view.back())),
            browser_toolbar_button(
                cx,
                "ios-browser-forward",
                IconName::ArrowRight,
                "Forward",
                !writable || !tab.forward,
                false,
            )
            .on_click(cx.listener(|this, _, _, _| this.tabs[this.active].view.forward())),
            browser_toolbar_button(
                cx,
                "ios-browser-reload",
                if tab.loading {
                    IconName::Xmark
                } else {
                    IconName::Redo2
                },
                "Reload",
                !writable,
                false,
            )
            .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            browser_address(&self.address, Icon::new(IconName::Globe), cx),
            browser_toolbar_button(
                cx,
                "ios-browser-element-picker",
                IconName::Inspector,
                if tab.picking {
                    "Cancel element picker"
                } else {
                    "Pick an element"
                },
                !writable || tab.loading,
                tab.picking,
            )
            .on_click(cx.listener(|this, _, window, cx| this.toggle_element_pick(window, cx))),
            div(),
        );
        let pane = self.pane;
        let actions = div().flex().gap_1().children(
            [
                (
                    "ios-browser-split-right",
                    IconName::PanelRight,
                    "Split right",
                    "split-window",
                    vec!["--kind".into(), "picker".into(), "-h".into()],
                ),
                (
                    "ios-browser-split-bottom",
                    IconName::PanelBottom,
                    "Split bottom",
                    "split-window",
                    vec!["--kind".into(), "picker".into(), "-v".into()],
                ),
                (
                    "ios-browser-close",
                    IconName::Xmark,
                    "Close pane",
                    "kill-pane",
                    vec![],
                ),
            ]
            .into_iter()
            .map(|(id, icon, label, command, mut args)| {
                args.extend(["-t".into(), pane.to_string()]);
                let connection = self.connection.clone();
                pane_header_icon_button(id, icon, writable, cx)
                    .tooltip(label)
                    .on_click(move |_, _, cx| {
                        connection.update(cx, |connection, cx| {
                            connection.command(command, args.clone(), cx)
                        });
                    })
            }),
        );
        let view = tab.view.clone();
        let visible = self.visible && self.profile.available();
        let radius = self.radii.bottom_left.min(self.radii.bottom_right);
        let error = if !self.profile.available() {
            Some("Waiting for the host connection…".to_owned())
        } else {
            tab.error.clone()
        };
        let picking = tab.picking;
        let pick_status = if picking {
            Some("Tap an element to copy its context and screenshot".to_owned())
        } else {
            tab.pick_status.clone()
        };
        div()
            .id(("ios-browser", pane.0))
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .rounded_tl(self.radii.top_left)
            .rounded_tr(self.radii.top_right)
            .bg(cx.theme().background.opaque())
            .text_color(cx.theme().foreground)
            .child(BrowserHeader::new(
                self.active_pane,
                strip,
                actions,
                toolbar,
            ))
            .when_some(pick_status, |pane, status| {
                pane.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .min_h(px(32.0))
                        .px_2()
                        .gap_2()
                        .text_sm()
                        .text_color(cx.theme().foreground.muted())
                        .child(status)
                        .when(picking, |row| {
                            row.child(
                                Button::new("ios-browser-cancel-picker")
                                    .label("Cancel")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_element_pick(window, cx)
                                    })),
                            )
                        }),
                )
            })
            .when_some(error, |pane, error| {
                pane.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(error),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .on_prepaint(move |bounds, window, _| {
                        view.mount(bounds, radius, window);
                        view.set_visible(
                            visible && bounds.size.width > px(0.0) && bounds.size.height > px(0.0),
                        );
                    }),
            )
    }
}

fn normalized(descriptor: &BrowserDescriptor) -> BrowserDescriptor {
    let mut descriptor = descriptor.clone();
    if descriptor.tabs.is_empty() {
        descriptor.tabs.push("about:blank".into());
    }
    descriptor.active_tab = descriptor.active_tab.min(descriptor.tabs.len() - 1);
    descriptor
}
