use std::{collections::VecDeque, rc::Rc, sync::Arc, time::Duration};

use crate::native_browser::{
    BrowserEvent, BrowserMount, BrowserProfile, BrowserView, ElementPickerAppearance,
};
use futures::StreamExt as _;
use zz_gpui::{
    AnyElement, App, Bounds, ClipboardEntry, ClipboardItem, ClipboardString, Context, Corners,
    ElementId, Entity, FocusHandle, Focusable, GlassMaterial, Hsla, Image, ImageFormat, Pixels,
    Render, RenderImage, Rgba, Subscription, TextInputMode, TouchDragEvent, TouchPhase, Window,
    canvas, div, point, prelude::*, px, size,
};
use zz_protocol::{BrowserCommand, BrowserDescriptor, PaneId};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Edge, ElementExt as _, Elevation, Icon,
    IconName, PaintState, StyledExt as _,
    browser::{
        BrowserEmptyHint, BrowserErrorPanel, BrowserHeader, BrowserTabInfo, BrowserTabStrip,
        BrowserToolbar, browser_address, browser_start_surface, browser_toolbar_button,
    },
    button::{Button, ButtonVariants as _},
    compact::{
        bottom_sheet, close_sheet, compact_tabs_button, floating_sheet, sheet_close, sheet_inset,
        sheet_row,
    },
    input::{InputEvent, InputState},
    pane::{PaneDrag, PaneDragSource, pane_header_icon_button},
    rems_from_px, roles,
};

use super::compact::PillNotice;
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
    held: bool,
    pick_status: Option<String>,
    _events: zz_gpui::Task<()>,
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
    compact: bool,
    tabs_sheet: bool,
    holding_pick: bool,
    pick_reached_page: bool,
    covered_bottom: Pixels,
    pill_hole: Option<(Bounds<Pixels>, Pixels)>,
    backdrop: Option<(u64, Bounds<Pixels>, Arc<RenderImage>, Option<Hsla>)>,
    radii: Corners<Pixels>,
    header_drag_handler: Option<Rc<dyn Fn(&PaneDrag, &mut Window, &mut App)>>,
    header_touch_drag_handler: Option<Rc<dyn Fn(&zz_gpui::TouchDragEvent, &mut Window, &mut App)>>,
    page_scroll_handler: Option<Rc<dyn Fn(f32, bool, &mut Window, &mut App)>>,
    notice_handler: Option<Rc<dyn Fn(&'static str, PillNotice, &mut Window, &mut App)>>,
    _subscriptions: Vec<Subscription>,
    _backdrop_tick: zz_gpui::Task<()>,
}

const BACKDROP_TICK: Duration = Duration::from_secs(1);

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
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search or enter address")
                .input_mode(TextInputMode::Url)
        });
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
            compact: false,
            tabs_sheet: false,
            holding_pick: false,
            pick_reached_page: false,
            covered_bottom: px(0.0),
            pill_hole: None,
            backdrop: None,
            radii: Corners::default(),
            header_drag_handler: None,
            header_touch_drag_handler: None,
            page_scroll_handler: None,
            notice_handler: None,
            _subscriptions: vec![submit, commands],
            _backdrop_tick: cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(BACKDROP_TICK).await;
                    if this.update(cx, |this, _| this.tick_backdrop()).is_err() {
                        break;
                    }
                }
            }),
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

    pub(super) fn set_header_drag_handler(
        &mut self,
        handler: impl Fn(&PaneDrag, &mut Window, &mut App) + 'static,
    ) {
        self.header_drag_handler = Some(Rc::new(handler));
    }

    pub(super) fn set_header_touch_drag_handler(
        &mut self,
        handler: impl Fn(&zz_gpui::TouchDragEvent, &mut Window, &mut App) + 'static,
    ) {
        self.header_touch_drag_handler = Some(Rc::new(handler));
    }

    /// Called as the shown page scrolls by hand: how far its content moved
    /// toward its end, and whether it reached its top.
    pub(super) fn set_page_scroll_handler(
        &mut self,
        handler: impl Fn(f32, bool, &mut Window, &mut App) + 'static,
    ) {
        self.page_scroll_handler = Some(Rc::new(handler));
    }

    pub(super) fn set_notice_handler(
        &mut self,
        handler: impl Fn(&'static str, PillNotice, &mut Window, &mut App) + 'static,
    ) {
        self.notice_handler = Some(Rc::new(handler));
    }

    /// The phone's floating pill over the page: how much of the page's
    /// bottom it covers and where it sits in the window, with its corner
    /// radius.
    pub(super) fn set_pill(
        &mut self,
        covered_bottom: Pixels,
        hole: Option<(Bounds<Pixels>, Pixels)>,
        cx: &mut Context<Self>,
    ) {
        if self.covered_bottom != covered_bottom || self.pill_hole != hole {
            self.covered_bottom = covered_bottom;
            self.pill_hole = hole;
            cx.notify();
        }
    }

    pub(super) fn backdrop_color(&self) -> Option<Hsla> {
        let (id, _, _, color) = self.backdrop.as_ref()?;
        let tab = &self.tabs[self.active];
        let shown = self.visible
            && self.profile.available()
            && tab.error.is_none()
            && tab.url != "about:blank"
            && !(self.compact && self.tabs_sheet);
        color.filter(|_| shown && self.pill_hole.is_some() && *id == tab.id)
    }

    fn tick_backdrop(&self) {
        if self.visible && self.pill_hole.is_some() {
            self.tabs[self.active].view.refresh_backdrop();
        }
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
        } else {
            self.start_element_pick(cx);
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn held_pick(
        &mut self,
        event: &TouchDragEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match event.phase {
            TouchPhase::Started => {
                if !self.writable(cx) || !self.visible || self.tabs[self.active].picking {
                    return false;
                }
                self.tabs[self.active].pick_status = None;
                self.holding_pick = self.start_element_pick(cx);
                self.tabs[self.active].held = self.holding_pick;
                self.pick_reached_page = false;
                if self.holding_pick {
                    self.focus.focus(window, cx);
                    cx.notify();
                }
                self.holding_pick
            }
            TouchPhase::Moved => {
                if self.holding_pick {
                    self.pick_reached_page |= self.tabs[self.active]
                        .view
                        .point_element_pick(event.position);
                }
                true
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if !std::mem::take(&mut self.holding_pick) {
                    return true;
                }
                let tab = &mut self.tabs[self.active];
                if event.phase == TouchPhase::Ended && self.pick_reached_page {
                    tab.view.point_element_pick(event.position);
                    tab.view.release_element_pick();
                } else {
                    tab.view.cancel_element_pick();
                    tab.picking = false;
                    if event.phase == TouchPhase::Ended {
                        self.pick_notice(
                            "Hold and drag onto the page",
                            PillNotice::Hint,
                            window,
                            cx,
                        );
                    }
                }
                cx.notify();
                true
            }
        }
    }

    fn pick_notice(
        &mut self,
        text: &'static str,
        kind: PillNotice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.notice_handler.clone().filter(|_| self.compact) {
            Some(handler) => handler(text, kind, window, cx),
            None => self.tabs[self.active].pick_status = Some(text.into()),
        }
    }

    #[allow(
        clippy::disallowed_methods,
        reason = "the picker overlay is page CSS, not chrome"
    )]
    fn start_element_pick(&mut self, cx: &mut Context<Self>) -> bool {
        let tab = &mut self.tabs[self.active];
        if tab.loading {
            return false;
        }
        let theme = cx.theme();
        let appearance = ElementPickerAppearance {
            highlight_outline: zz_ui::to_hex(theme.foreground),
            highlight_fill: zz_ui::to_hex(theme.foreground.fill()),
            highlight_contrast: zz_ui::to_hex(theme.background.opaque()),
            preview_background: zz_ui::to_hex(theme.surface(Elevation::Panel).fill.opaque()),
            preview_foreground: zz_ui::to_hex(theme.foreground),
            preview_border: zz_ui::to_hex(theme.border()),
            shadow: theme
                .casts_shadow(zz_ui::SurfaceRole::POPOVER)
                .then(|| zz_ui::to_hex(theme.shadow_color)),
            radius: f32::from(theme.radius),
            font_family: theme.mono_font_family.to_string(),
            page_zoom: 1.0,
        };
        match tab.view.start_element_pick(&appearance) {
            Ok(()) => tab.picking = true,
            Err(error) => tab.pick_status = Some(error),
        }
        tab.held = false;
        tab.picking
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
            held: false,
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
                self.pick_notice(status, PillNotice::Done, window, cx);
            }
            BrowserEvent::ElementPickCancelled => self.tabs[index].picking = false,
            BrowserEvent::ElementPickFailed => {
                self.tabs[index].picking = false;
                if index == self.active {
                    self.pick_notice(
                        "Could not inspect that element. Try again.",
                        PillNotice::Failed,
                        window,
                        cx,
                    );
                }
            }
            BrowserEvent::Backdrop { bounds, image } => {
                let hole = self.pill_hole.filter(|_| index == self.active);
                let next = hole.map(|(hole, _)| {
                    let color = average_under(&image, bounds, hole);
                    (self.tabs[index].id, bounds, image, color)
                });
                if let Some((_, _, old, _)) = std::mem::replace(&mut self.backdrop, next) {
                    let _ = window.drop_image(old);
                }
            }
            BrowserEvent::Refresh => {
                if index == self.active {
                    self.refresh(cx);
                } else {
                    self.tabs[index].view.end_refresh();
                }
            }
            BrowserEvent::Open(url) => {
                if self.writable(cx) {
                    self.add_tab(url, window, cx);
                }
            }
            BrowserEvent::Scrolled { travel, at_top } => {
                if index == self.active
                    && self.visible
                    && let Some(handler) = self.page_scroll_handler.clone()
                {
                    handler(travel, at_top, window, cx);
                }
                return;
            }
            BrowserEvent::Focused => {
                self.focus.focus(window, cx);
                self.connection.update(cx, |connection, cx| {
                    connection.command(
                        "select-pane",
                        vec!["-Z".into(), "-t".into(), self.pane.to_string()],
                        cx,
                    );
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
        compact: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.active_pane != active
            || self.radii != radii
            || self.compact != compact
            || normalized(descriptor) != self.received;
        self.active_pane = active;
        self.radii = radii;
        self.compact = compact;
        self.tabs_sheet &= compact;
        if !compact {
            self.covered_bottom = px(0.0);
            self.pill_hole = None;
        }
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
                            tab.url.clone_from(url);
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

    pub(super) fn shown_page(&self) -> (&str, &str) {
        let tab = &self.tabs[self.active];
        (&tab.title, &tab.url)
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

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let writable = self.writable(cx);
        let tab = &mut self.tabs[self.active];
        if !writable {
            tab.view.end_refresh();
            return;
        }
        tab.pick_status = None;
        if tab.error.take().is_some() {
            tab.pending_load = true;
        }
        if tab.pending_load {
            self.resume();
        } else {
            tab.view.reload();
        }
        let tab = &self.tabs[self.active];
        if tab.error.is_some() {
            tab.view.end_refresh();
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
            connection.command("set-browser-tabs", args, cx);
        });
    }
}

impl Focusable for BrowserPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BrowserPane {
    fn tab_row(&self, writable: bool, cx: &mut Context<Self>) -> (BrowserTabStrip, zz_gpui::Div) {
        let tabs = self
            .tabs
            .iter()
            .map(|tab| BrowserTabInfo::new(tab.id, tab_title(tab).to_owned(), tab.url.clone()))
            .collect();
        let pane = self.pane;
        let (can_drag, drag_title) = {
            let snapshot = self.connection.read(cx).core.snapshot();
            let pane_window = snapshot
                .sessions
                .iter()
                .flat_map(|session| &session.windows)
                .find(|window| window.panes.contains_key(&pane));
            (
                writable
                    && self.header_drag_handler.is_some()
                    && pane_window.is_some_and(|window| {
                        window.zoomed_pane.is_none() && window.panes.len() > 1
                    }),
                pane_window
                    .and_then(|window| window.panes.get(&pane))
                    .map_or_else(|| "Browser".to_owned(), zz_client::navigation::pane_label),
            )
        };
        let floats =
            zz_client::floating::pane_floats(self.connection.read(cx).core.snapshot(), pane);
        let drag_handler = self.header_drag_handler.clone();
        let mut drag = PaneDragSource::new(
            pane,
            drag_title,
            can_drag || (writable && floats),
            move |drag, window, cx| {
                if let Some(handler) = &drag_handler {
                    handler(drag, window, cx);
                }
            },
        )
        .floating(floats);
        if let Some(handler) = self.header_touch_drag_handler.clone() {
            drag = drag.touch(super::touch_drag_handle(move |event, window, cx| {
                handler(event, window, cx);
            }));
        }
        let close_connection = self.connection.clone();
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
                    this.add_tab("about:blank".into(), window, cx);
                });
            })
            .title(drag, move |_, cx| {
                if !writable {
                    return;
                }
                close_connection.update(cx, |connection, cx| {
                    connection.command("kill-pane", vec!["-t".to_owned(), pane.to_string()], cx);
                });
            });
        let actions =
            div()
                .flex()
                .gap_1()
                .children(zz_ui::pane::pane_header_actions(floats).iter().map(
                    |(action, icon, label)| {
                        let connection = self.connection.clone();
                        let commands = action.commands(pane);
                        pane_header_icon_button(
                            ("ios-browser-action", *action as usize),
                            icon.clone(),
                            writable,
                            cx,
                        )
                        .tooltip(*label)
                        .on_click(move |_, _, cx| {
                            connection.update(cx, |connection, cx| {
                                for command in commands.clone() {
                                    connection.invoke(command, cx);
                                }
                            });
                        })
                    },
                ));
        (strip, actions)
    }

    fn compact_tab_row(
        &self,
        writable: bool,
        cx: &mut Context<Self>,
    ) -> (zz_gpui::Div, zz_gpui::Div) {
        let pane = self.pane;
        let title = div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_2()
            .pl_1()
            .child(
                Icon::new(IconName::Globe)
                    .size(rems_from_px(14.0))
                    .flex_none(),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(tab_title(&self.tabs[self.active]).to_owned()),
            );
        let connection = self.connection.clone();
        let actions = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                compact_tabs_button(("ios-browser-tabs", pane.0), self.tabs.len(), cx)
                    .disabled(!writable)
                    .hit_slop(4.0, 10.0)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tabs_sheet = true;
                        cx.notify();
                    })),
            )
            .child(
                pane_header_icon_button(
                    ("ios-browser-close", pane.0),
                    IconName::Xmark,
                    writable,
                    cx,
                )
                .hit_slop(4.0, 10.0)
                .on_click(move |_, _, cx| {
                    connection.update(cx, |connection, cx| {
                        connection.command("kill-pane", vec!["-t".into(), pane.to_string()], cx);
                    });
                }),
            );
        (title, actions)
    }

    fn tab_drawer(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let sheet = ElementId::from(("ios-browser-tabs-sheet", self.pane.0));
        let muted = cx.theme().foreground.muted();
        let closable = self.tabs.len() > 1;
        let rows: Vec<AnyElement> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let id = tab.id;
                let picked = sheet.clone();
                sheet_row(
                    ("ios-browser-tab", id),
                    roles::PHONE_ROW,
                    (index == self.active).then(|| Icon::new(IconName::Check)),
                    tab_title(tab).to_owned(),
                    cx,
                )
                .child(
                    div()
                        .flex_none()
                        .max_w(px(120.0))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(13.0))
                        .text_color(muted)
                        .child(host(&tab.url).to_owned()),
                )
                .when(closable, |row| {
                    row.child(
                        pane_header_icon_button(
                            ("ios-browser-tab-close", id),
                            IconName::Xmark,
                            true,
                            cx,
                        )
                        .hit_slop(8.0, 10.0)
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_tab(id, window, cx);
                            },
                        )),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select(id, window, cx);
                    close_sheet(picked.clone(), cx);
                }))
                .into_any_element()
            })
            .collect();
        let opened = sheet.clone();
        let new_tab = sheet_row(
            ("ios-browser-new-tab", self.pane.0),
            roles::PHONE_ROW,
            Some(Icon::new(IconName::Plus)),
            "New tab",
            cx,
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            this.add_tab("about:blank".into(), window, cx);
            close_sheet(opened.clone(), cx);
        }));
        let closed = sheet.clone();
        let close = sheet_close(("ios-browser-tabs-close", self.pane.0))
            .on_click(move |_, _, cx| close_sheet(closed.clone(), cx));
        let dismiss = cx.weak_entity();
        floating_sheet(
            bottom_sheet(
                sheet,
                "Tabs",
                [close.into_any_element()],
                div()
                    .flex()
                    .flex_col()
                    .list_inset(cx)
                    .pb(px(8.0))
                    .children(rows)
                    .child(
                        div()
                            .mx(px(12.0))
                            .my(px(4.0))
                            .rule_on(roles::LIST_RULE, Edge::Top, cx),
                    )
                    .child(new_tab),
                sheet_inset(window),
                move |_, cx| {
                    let _ = dismiss.update(cx, |this, cx| {
                        this.tabs_sheet = false;
                        cx.notify();
                    });
                },
            )
            .surface_role(roles::TABS_DRAWER)
            .header_role(roles::PHONE_BAR),
            window,
        )
    }
}

impl Render for BrowserPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let writable = self.writable(cx);
        let pane = self.pane;
        let tab = &self.tabs[self.active];
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
            browser_address(&self.address, Icon::new(IconName::Globe), window, cx),
            div()
                .relative()
                .flex_none()
                .child(
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
                    .on_click(
                        cx.listener(|this, _, window, cx| this.toggle_element_pick(window, cx)),
                    ),
                )
                .child(super::touch_drag_handle({
                    let pane = cx.weak_entity();
                    move |event, window, cx| {
                        let claimed = pane
                            .update(cx, |this, cx| this.held_pick(event, window, cx))
                            .unwrap_or(false);
                        if claimed && event.phase == TouchPhase::Started {
                            window.prevent_default();
                        }
                    }
                })),
            div(),
        );
        let header = if self.compact {
            let (title, actions) = self.compact_tab_row(writable, cx);
            BrowserHeader::new(true, title, actions, toolbar.back_only())
        } else {
            let (strip, actions) = self.tab_row(writable, cx);
            BrowserHeader::new(self.active_pane, strip, actions, toolbar)
        };
        let drawer = self.compact && self.tabs_sheet;
        let covered_bottom = self.covered_bottom;
        let hole = self.pill_hole;
        let reach = hole.and_then(|_| {
            cx.theme()
                .layer(roles::PHONE_PILL, PaintState::Rest)
                .glass
                .filter(GlassMaterial::reads_backdrop)
                .map(|glass| glass.backdrop_reach())
        });
        let tab_drawer = drawer.then(|| self.tab_drawer(window, cx));
        let tab = &self.tabs[self.active];
        let view = tab.view.clone();
        let backdrop = self
            .backdrop
            .clone()
            .filter(|(id, _, _, _)| reach.is_some() && *id == tab.id);
        let visible = self.visible && self.profile.available();
        let theme = cx.theme();
        let edge = theme.pane_edge_width();
        let radius = (self.radii.bottom_left.min(self.radii.bottom_right) - edge).max(px(0.0));
        let waiting = !self.profile.available();
        let error = tab.error.clone().filter(|_| !waiting);
        let failed = error.is_some();
        let blank = !failed && !waiting && tab.url == "about:blank";
        let retry = self.writable(cx).then(|| {
            Button::new("ios-browser-retry")
                .primary()
                .on(zz_ui::feedback::alert_ground(cx))
                .label("Try again")
                .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
        });
        let picking = tab.picking && !tab.held;
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
            .text_color(cx.theme().foreground)
            .child(header)
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
                                    .ghost()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_element_pick(window, cx);
                                    })),
                            )
                        }),
                )
            })
            .when(waiting, |pane| {
                pane.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_sm()
                        .text_color(cx.theme().foreground.muted())
                        .child("Waiting for the host connection…"),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .on_prepaint(move |bounds, window, _| {
                        let bounds = Bounds::new(
                            point(bounds.left() + edge, bounds.top()),
                            size(
                                (bounds.size.width - edge * 2.0).max(px(0.0)),
                                (bounds.size.height - edge).max(px(0.0)),
                            ),
                        );
                        let backdrop = reach.map(|reach| {
                            let top = (bounds.bottom() - covered_bottom - reach).max(bounds.top());
                            Bounds::from_corners(point(bounds.left(), top), bounds.bottom_right())
                        });
                        view.mount(
                            BrowserMount {
                                bounds,
                                radius,
                                covered_bottom,
                                hole,
                                backdrop,
                            },
                            window,
                        );
                        view.set_visible(
                            visible
                                && !failed
                                && !drawer
                                && !blank
                                && bounds.size.width > px(0.0)
                                && bounds.size.height > px(0.0),
                        );
                    })
                    .when_some(
                        backdrop.filter(|_| !failed && !drawer && !blank),
                        |page, (_, bounds, image, _)| {
                            page.child(
                                canvas(
                                    |_, _, _| {},
                                    move |_, (), window, _| {
                                        let _ = window.paint_image(
                                            bounds,
                                            bounds,
                                            Corners::default(),
                                            image,
                                            0,
                                            false,
                                        );
                                    },
                                )
                                .absolute()
                                .size_full(),
                            )
                        },
                    )
                    .when(blank, |page| {
                        page.child(browser_start_surface(BrowserEmptyHint))
                    })
                    .when_some(error, |page, error| {
                        let mut panel = BrowserErrorPanel::new(error);
                        if let Some(retry) = retry {
                            panel = panel.retry(retry);
                        }
                        page.child(browser_start_surface(panel))
                    }),
            )
            .children(tab_drawer)
    }
}

fn average_under(image: &RenderImage, strip: Bounds<Pixels>, hole: Bounds<Pixels>) -> Option<Hsla> {
    let bytes = image.as_bytes(0)?;
    let size = image.size(0);
    let (width, height) = (
        usize::try_from(size.width.0).ok()?,
        usize::try_from(size.height.0).ok()?,
    );
    let area = hole.intersect(&strip);
    let column = |at: Pixels| {
        ((f32::from(at - strip.left()) / f32::from(strip.size.width) * width as f32) as usize)
            .min(width)
    };
    let row = |at: Pixels| {
        ((f32::from(at - strip.top()) / f32::from(strip.size.height) * height as f32) as usize)
            .min(height)
    };
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for y in (row(area.top())..row(area.bottom())).step_by(2) {
        for x in (column(area.left())..column(area.right())).step_by(2) {
            let pixel = bytes.get((y * width + x) * 4..(y * width + x) * 4 + 3)?;
            sum[0] += u64::from(pixel[2]);
            sum[1] += u64::from(pixel[1]);
            sum[2] += u64::from(pixel[0]);
            count += 1;
        }
    }
    let channel = |sum: u64| sum as f32 / count as f32 / 255.0;
    (count > 0).then(|| {
        Hsla::from(Rgba {
            r: channel(sum[0]),
            g: channel(sum[1]),
            b: channel(sum[2]),
            a: 1.0,
        })
    })
}

fn tab_title(tab: &Tab) -> &str {
    if !tab.title.is_empty() {
        &tab.title
    } else if tab.url == "about:blank" {
        "New tab"
    } else {
        &tab.url
    }
}

pub(super) fn host(url: &str) -> &str {
    if url == "about:blank" {
        return "";
    }
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
}

fn normalized(descriptor: &BrowserDescriptor) -> BrowserDescriptor {
    let mut descriptor = descriptor.clone();
    if descriptor.tabs.is_empty() {
        descriptor.tabs.push("about:blank".into());
    }
    descriptor.active_tab = descriptor.active_tab.min(descriptor.tabs.len() - 1);
    descriptor
}
