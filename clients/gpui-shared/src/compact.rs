use std::{collections::BTreeSet, sync::Arc, time::Duration};

use gpui::{
    AnyElement, App, Context, Corners, Entity, Focusable as _, IntoElement, Keystroke,
    ParentElement as _, PinchEvent, Pixels, ScrollDelta, ScrollWheelEvent, SharedString, Stateful,
    Styled as _, Subscription, TouchPhase, Window, div, prelude::*, px,
};
use zz_client::StatusBarModel;
use zz_protocol::{InputMessage, PaneId, PaneKindSnapshot, WindowId};
use zz_terminal::KeyAction;
use zz_ui::{
    ActiveTheme as _, IconName,
    compact::{
        ArrowPadEvent, COMPACT_BAR_HEIGHT, Instant, KEY_ROW_HEIGHT, KeyRow, PageDot, Pager,
        PagerEvent, PopoverKey, PopoverKeyEvent, PopoverKeyItem, ToolKeys, WhichKeyList,
        bottom_sheet, compact_bar, compact_bar_button, compact_bar_title, compact_hud,
        compact_pane_header, page_dots, top_shade,
    },
    kbd::Kbd,
    pane::pane_header_icon_button,
    rems_from_px,
    which_key::{WhichKeyCap, WhichKeyRow},
};

use super::{AppShell, sidebar};
use crate::terminal::TerminalDisplayPreferences;

const PAGE_GAP: f32 = 12.0;
const CARD_RADIUS: f32 = 22.0;
const PINCH_STEPS: f32 = 20.0;
const SCALE_HUD_LINGER: Duration = Duration::from_secs(1);

const NEW_PANE: &str = "new-pane";
const NEW_WINDOW: &str = "new-window";
const RENAME_PANE: &str = "rename-pane";
const LAST_PANE: &str = "last-pane";
const KILL_PANE: &str = "kill-pane";
const ALL_BINDINGS: &str = "all-bindings";

#[derive(Default)]
pub(super) struct CompactState {
    pager: Pager,
    width: f32,
    tool_keys: Option<(ToolKeys, [Subscription; 4])>,
    bindings: bool,
    preview: Option<bool>,
    zoom_intent: Option<PaneId>,
    compact_keyboard: Option<bool>,
    last_current: Option<PaneId>,
    previous: Option<PaneId>,
    pinch: Option<(f32, f32)>,
    scale_hud: Option<Instant>,
    safe_bottom: Pixels,
    keyboard_overlap: Pixels,
    keyboard_moving: bool,
}

impl CompactState {
    pub(super) fn reset_attachment(&mut self) {
        self.preview = None;
        self.zoom_intent = None;
        self.last_current = None;
        self.previous = None;
        self.bindings = false;
    }
}

fn prefix_menu() -> (Vec<PopoverKeyItem>, PopoverKeyItem) {
    (
        vec![
            PopoverKeyItem::new(NEW_PANE, "New pane").icon(IconName::Plus),
            PopoverKeyItem::new(NEW_WINDOW, "New window").icon(IconName::AppWindow),
            PopoverKeyItem::new(RENAME_PANE, "Rename pane").icon(IconName::Pencil),
            PopoverKeyItem::new(LAST_PANE, "Last pane").icon(IconName::History),
            PopoverKeyItem::new(KILL_PANE, "Kill pane")
                .icon(IconName::Xmark)
                .danger(),
        ],
        PopoverKeyItem::new(ALL_BINDINGS, "All bindings").icon(IconName::Keyboard),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Page {
    pub pane: PaneId,
    pub window: WindowId,
    pub kind: PaneKindSnapshot,
    pub title: SharedString,
    pub attention: bool,
}

pub(super) fn pages(model: &StatusBarModel, unseen: &BTreeSet<PaneId>) -> (Vec<Page>, usize) {
    let mut pages = Vec::new();
    let mut current = 0;
    for window in &model.windows {
        for pane in &window.panes {
            if window.active && pane.active {
                current = pages.len();
            }
            pages.push(Page {
                pane: pane.id,
                window: window.id,
                kind: pane.kind.clone(),
                title: pane.label.clone().into(),
                attention: unseen.contains(&pane.id)
                    || (!window.active && (window.bell || window.activity)),
            });
        }
    }
    (pages, current)
}

pub(super) fn pinch_font_scale(start: f32, pinch: f32) -> f32 {
    let scale = (start * pinch * PINCH_STEPS).round() / PINCH_STEPS;
    if scale.is_finite() {
        scale.clamp(0.5, 3.0)
    } else {
        start
    }
}

pub(super) fn dot_groups(pages: &[Page], current: usize) -> Vec<Vec<PageDot>> {
    let mut groups: Vec<Vec<PageDot>> = Vec::new();
    let mut previous = None;
    for (index, page) in pages.iter().enumerate() {
        if previous != Some(page.window) {
            groups.push(Vec::new());
            previous = Some(page.window);
        }
        if let Some(group) = groups.last_mut() {
            group.push(PageDot {
                active: index == current,
                attention: page.attention && index != current,
            });
        }
    }
    groups
}

pub(super) const fn kind_icon(kind: &PaneKindSnapshot) -> IconName {
    match kind {
        PaneKindSnapshot::Terminal => IconName::SquareTerminal,
        PaneKindSnapshot::Browser(_) => IconName::Globe,
        PaneKindSnapshot::Agent(_) => IconName::Bot,
        PaneKindSnapshot::Editor(_) => IconName::File,
        PaneKindSnapshot::Picker => IconName::Plus,
    }
}

pub(super) fn tmux_keystroke(spelling: &str) -> Option<Keystroke> {
    let key = zz_client::ChromeKey::parse(spelling)?;
    let base = match key.base.as_str() {
        " " | "Space" => "space",
        "Enter" => "enter",
        "Escape" => "escape",
        "Tab" => "tab",
        "BSpace" => "backspace",
        "DC" => "delete",
        "Up" => "up",
        "Down" => "down",
        "Left" => "left",
        "Right" => "right",
        "Home" => "home",
        "End" => "end",
        "PPage" | "PageUp" => "pageup",
        "NPage" | "PageDown" => "pagedown",
        "-" => "minus",
        other if other.chars().count() == 1 => other,
        _ => return None,
    };
    let mut source = String::new();
    if key.command {
        source.push_str("cmd-");
    }
    if key.control {
        source.push_str("ctrl-");
    }
    if key.alt {
        source.push_str("alt-");
    }
    let mut characters = base.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) if character.is_ascii_uppercase() => {
            source.push_str("shift-");
            source.extend(character.to_lowercase());
        }
        _ => {
            if key.shift {
                source.push_str("shift-");
            }
            source.push_str(base);
        }
    }
    let mut keystroke = Keystroke::parse(&source).ok()?;
    if base == "minus" {
        keystroke.key = "-".into();
    }
    if keystroke.key_char.is_none()
        && !keystroke.modifiers.control
        && !keystroke.modifiers.alt
        && !keystroke.modifiers.platform
        && base.chars().count() == 1
    {
        keystroke.key_char = Some(base.to_owned());
    }
    Some(keystroke)
}

fn dispatch_key(keystroke: Keystroke, window: &mut Window, cx: &mut App) {
    window.defer(cx, move |window, cx| {
        window.dispatch_keystroke(keystroke, cx);
    });
}

fn send_chord(
    _: &mut AppShell,
    _: &Entity<PopoverKey>,
    event: &PopoverKeyEvent,
    window: &mut Window,
    cx: &mut Context<AppShell>,
) {
    let PopoverKeyEvent::Pick(id) = event;
    if let Ok(keystroke) = Keystroke::parse(id) {
        let keystroke = zz_ui::compact::StickyModifiers::apply(&keystroke, cx);
        dispatch_key(keystroke, window, cx);
    }
}

fn key_row_lift(safe_bottom: Pixels, bar: Pixels, overlap: Pixels) -> Pixels {
    (safe_bottom + bar - px(KEY_ROW_HEIGHT) - overlap).max(px(0.0))
}

fn keyboard_visible(window: &Window) -> bool {
    window.visual_viewport_bounds().size.height + px(1.0) < window.viewport_size().height
}

fn prefix_spelling(core: &zz_client::ClientCore) -> String {
    core.mux_options()
        .get(zz_protocol::MuxOptionKey::Prefix)
        .filter(|option| !option.value.eq_ignore_ascii_case("none"))
        .map_or_else(
            || "C-b".to_owned(),
            |option| zz_protocol::canonical_key(&option.value),
        )
}

fn binding_rows(core: &zz_client::ClientCore, prefix: &str) -> Arc<[WhichKeyRow]> {
    let cap = |keys: &[String], yours: bool| WhichKeyCap {
        keys: keys
            .iter()
            .map(|key| tmux_keystroke(key))
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default(),
        raw: keys.join(" ").into(),
        yours,
    };
    zz_client::which_key::rows(core.key_tables(), "prefix", prefix)
        .into_iter()
        .filter(|row| tmux_keystroke(row.first_key()).is_some())
        .map(|row| WhichKeyRow {
            id: row.first_key().to_owned().into(),
            caps: row
                .keys
                .iter()
                .map(|set| cap(&set.keys, set.yours))
                .collect(),
            label: row.label.into(),
            group: row
                .group
                .map(|group| SharedString::new_static(group.title())),
            repeat: row.repeat,
        })
        .collect()
}

impl AppShell {
    pub(super) fn compact_active(&self, window: &Window, cx: &App) -> bool {
        Self::narrow(window) && self.settings.is_none() && self.active_window(cx).is_some()
    }

    pub(super) fn sync_compact_mode(&mut self, compact: bool, cx: &mut Context<Self>) {
        if self.compact.preview != Some(compact) && self.connection.read(cx).connected {
            self.compact.preview = Some(compact);
            self.connection.update(cx, |connection, cx| {
                connection.send(
                    zz_protocol::ProtocolMessage::SetTerminalPreview { enabled: compact },
                    cx,
                );
            });
            if compact {
                self.compact.zoom_intent = self.active_window(cx).map(|window| window.active_pane);
            }
        }
        if self.compact.compact_keyboard != Some(compact) {
            self.compact.compact_keyboard = Some(compact);
            #[cfg(target_os = "ios")]
            zz_gpui_ios::set_compact_keyboard(compact);
        }
        if !compact {
            zz_ui::compact::StickyModifiers::take(cx);
            if self.compact.pinch.take().is_some() {
                self.terminal_resize_suppressed.set(false);
                self.preferences.save();
            }
        }
    }

    pub(super) fn sync_keyboard_motion(&mut self, window: &mut Window) {
        let overlap = window.viewport_size().height - window.visual_viewport_bounds().bottom();
        if self.compact.keyboard_overlap != overlap {
            self.compact.keyboard_overlap = overlap;
            self.compact.keyboard_moving = true;
            self.terminal_resize_suppressed.set(true);
            window.request_animation_frame();
        } else if std::mem::take(&mut self.compact.keyboard_moving)
            && self.split_drag.is_none()
            && self.compact.pinch.is_none()
            && !self.compact.pager.is_moving()
        {
            self.terminal_resize_suppressed.set(false);
        }
    }

    fn compact_select(&mut self, page: &Page, cx: &mut Context<Self>) {
        let target = page.pane.to_string();
        if self
            .active_window(cx)
            .is_none_or(|window| window.id != page.window)
        {
            self.command("select-window", vec!["-t".into(), target.clone()], cx);
        }
        self.command("select-pane", vec!["-Z".into(), "-t".into(), target], cx);
        self.compact.zoom_intent = Some(page.pane);
        self.focused_pane = None;
    }

    fn commit_page(&mut self, event: Option<PagerEvent>, cx: &mut Context<Self>) {
        let Some(PagerEvent::Commit { index }) = event else {
            return;
        };
        let model = self.status_model(cx);
        let (pages, _) = pages(&model, &self.unseen_agents);
        if let Some(page) = pages.get(index) {
            self.compact_select(page, cx);
        }
    }

    fn reconcile_compact_zoom(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.compact.zoom_intent else {
            return;
        };
        let Some(active) = self.active_window(cx) else {
            return;
        };
        if active.active_pane != target {
            return;
        }
        self.compact.zoom_intent = None;
        let connection = self.connection.read(cx);
        if active.panes.len() > 1
            && active.zoomed_pane.is_none()
            && connection.connected
            && !connection.core.attached_read_only()
        {
            self.command(
                "resize-pane",
                vec!["-Z".into(), "-t".into(), target.to_string()],
                cx,
            );
        }
    }

    fn tool_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) -> ToolKeys {
        if let Some((keys, _)) = &self.compact.tool_keys {
            return keys.clone();
        }
        let (items, footer) = prefix_menu();
        let keys = ToolKeys::new(items, Some(footer), cx);
        let subscriptions = [
            cx.subscribe_in(
                &keys.arrows,
                window,
                |_, _, event: &ArrowPadEvent, window, cx| {
                    let ArrowPadEvent::Arrow(direction) = event;
                    if let Ok(keystroke) = Keystroke::parse(direction.key()) {
                        let keystroke = zz_ui::compact::StickyModifiers::apply(&keystroke, cx);
                        dispatch_key(keystroke, window, cx);
                    }
                },
            ),
            cx.subscribe_in(&keys.control, window, send_chord),
            cx.subscribe_in(&keys.alt, window, send_chord),
            cx.subscribe_in(
                &keys.prefix,
                window,
                |this, _, event: &PopoverKeyEvent, _, cx| {
                    let PopoverKeyEvent::Pick(id) = event;
                    this.prefix_action(id, cx);
                },
            ),
        ];
        self.compact.tool_keys = Some((keys.clone(), subscriptions));
        keys
    }

    fn prefix_action(&mut self, id: &str, cx: &mut Context<Self>) {
        let connection = self.connection.read(cx);
        if !connection.connected || connection.core.attached_read_only() {
            return;
        }
        let Some(pane) = self.active_window(cx).map(|window| window.active_pane) else {
            return;
        };
        let target = pane.to_string();
        let here = || vec!["-c".to_owned(), "#{pane_current_path}".to_owned()];
        match id {
            NEW_PANE => {
                let mut args = vec!["-h".to_owned(), "-t".to_owned(), target];
                args.extend(here());
                self.command("split-window", args, cx);
            }
            NEW_WINDOW => self.command("new-window", here(), cx),
            RENAME_PANE => {
                let title = self
                    .connection
                    .read(cx)
                    .core
                    .snapshot()
                    .sessions
                    .iter()
                    .flat_map(|session| &session.windows)
                    .find_map(|window| window.panes.get(&pane))
                    .map(|pane| pane.title.clone())
                    .unwrap_or_default();
                self.command(
                    "command-prompt",
                    vec![
                        "-I".into(),
                        title,
                        "-p".into(),
                        "rename pane:".into(),
                        format!("select-pane -t {target} -T '%%'"),
                    ],
                    cx,
                );
            }
            LAST_PANE => {
                let model = self.status_model(cx);
                let (pages, _) = pages(&model, &self.unseen_agents);
                if let Some(page) = self
                    .compact
                    .previous
                    .and_then(|previous| pages.iter().find(|page| page.pane == previous))
                {
                    self.compact_select(page, cx);
                }
            }
            KILL_PANE => self.command("kill-pane", vec!["-t".into(), target], cx),
            ALL_BINDINGS => self.compact.bindings = true,
            _ => {}
        }
        cx.notify();
    }

    fn send_binding(&self, key: &str, cx: &mut Context<Self>) {
        let Some(pane) = self.active_window(cx).map(|window| window.active_pane) else {
            return;
        };
        let core = &self.connection.read(cx).core;
        let mut spellings = Vec::new();
        if !core.prefix_armed() {
            spellings.push(prefix_spelling(core));
        }
        spellings.push(key.to_owned());
        let Some(keystrokes) = spellings
            .iter()
            .map(|spelling| tmux_keystroke(spelling))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        for keystroke in keystrokes {
            for action in [KeyAction::Press, KeyAction::Release] {
                self.send_input(
                    InputMessage::Key {
                        pane,
                        input: crate::terminal::keystroke_input(&keystroke, action),
                        text_follows: false,
                    },
                    cx,
                );
            }
        }
    }

    fn compact_scroll(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta_x = match event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.x),
            ScrollDelta::Lines(delta) => delta.x * 20.0,
        };
        let response = self.compact.pager.scroll(
            delta_x,
            event.touch_phase,
            self.compact.width,
            Instant::now(),
        );
        if !response.consumed {
            return;
        }
        cx.stop_propagation();
        if self.compact.pager.swallows_momentum() {
            window.end_touch_momentum();
        }
        self.terminal_resize_suppressed
            .set(self.compact.pager.is_moving());
        let changed = response.changed || response.event.is_some();
        self.commit_page(response.event, cx);
        if changed {
            cx.notify();
        }
    }

    fn compact_pinch(&mut self, event: &PinchEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.phase {
            TouchPhase::Started => {
                let model = self.status_model(cx);
                let (pages, _) = pages(&model, &self.unseen_agents);
                if !pages
                    .get(self.compact.pager.index())
                    .is_some_and(|page| page.kind == PaneKindSnapshot::Terminal)
                {
                    return;
                }
                self.compact.pinch = Some((self.preferences.terminal_font_scale, 1.0));
                self.terminal_resize_suppressed.set(true);
            }
            TouchPhase::Moved => {
                let Some((start, pinch)) = self.compact.pinch.as_mut() else {
                    return;
                };
                *pinch += event.delta;
                let scale = pinch_font_scale(*start, *pinch);
                if scale != self.preferences.terminal_font_scale {
                    self.preferences.terminal_font_scale = scale;
                    cx.set_global(TerminalDisplayPreferences {
                        font_family: self.preferences.terminal_font_family.clone(),
                        font_scale: scale,
                    });
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if self.compact.pinch.take().is_none() {
                    return;
                }
                self.preferences.save();
                self.sync_terminal_scale_input(window, cx);
                self.terminal_resize_suppressed.set(false);
                self.compact.scale_hud = Some(Instant::now() + SCALE_HUD_LINGER);
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(SCALE_HUD_LINGER).await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
                .detach();
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn status_model(&self, cx: &App) -> StatusBarModel {
        let core = &self.connection.read(cx).core;
        StatusBarModel::from_snapshot(
            core.snapshot(),
            core.attached_session(),
            None,
            self.preferences.status_bar_settings(),
        )
    }

    fn compact_page(
        &mut self,
        slot: &Page,
        current: bool,
        motion: f32,
        top_inset: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let radius = px(CARD_RADIUS * motion);
        let pane_id = slot.pane;
        let snapshot = self
            .connection
            .read(cx)
            .core
            .snapshot()
            .sessions
            .iter()
            .flat_map(|session| &session.windows)
            .find_map(|window| window.panes.get(&pane_id).cloned());
        let Some(pane) = snapshot else {
            return div().size_full().into_any_element();
        };
        let can_focus = current && self.compact_can_focus(window, cx);
        let dead = pane.dead.then(|| {
            pane.dead_status
                .map_or_else(|| "Dead".to_owned(), |status| format!("Dead · {status}"))
        });
        let mut background = cx.theme().background;
        let content = match &pane.kind {
            PaneKindSnapshot::Terminal => {
                let terminal = self.terminal_entity(pane_id, cx);
                if can_focus && self.focused_pane != Some(pane_id) {
                    terminal.read(cx).focus_handle(cx).focus(window, cx);
                    self.focused_pane = Some(pane_id);
                }
                terminal.update(cx, |terminal, cx| {
                    terminal.set_text_dimmed(false, 1.0, cx);
                    terminal.set_pane_status(dead.clone(), pane.synchronized_input, false, cx);
                    terminal.set_corner_radii(
                        Corners {
                            top_left: px(0.0),
                            top_right: px(0.0),
                            bottom_left: radius,
                            bottom_right: radius,
                        },
                        cx,
                    );
                });
                background = terminal.read(cx).pane_background(cx);
                terminal.into_any_element()
            }
            PaneKindSnapshot::Agent(descriptor) => {
                let agent = self.agent_entity(pane_id, descriptor, window, cx);
                agent.update(cx, |agent, cx| {
                    agent.update_descriptor(descriptor, cx);
                    agent.set_corner_radii(Corners::all(radius), cx);
                });
                if can_focus && self.focused_pane != Some(pane_id) {
                    agent.read(cx).focus_handle(cx).focus(window, cx);
                    self.focused_pane = Some(pane_id);
                }
                agent.into_any_element()
            }
            #[cfg(target_os = "ios")]
            PaneKindSnapshot::Browser(descriptor) => {
                let browser = self.browser_entity(pane_id, descriptor, window, cx);
                browser.update(cx, |browser, cx| {
                    browser.synchronize(descriptor, current, Corners::all(radius), window, cx);
                });
                if can_focus && self.focused_pane != Some(pane_id) {
                    browser.read(cx).focus_handle(cx).focus(window, cx);
                    self.focused_pane = Some(pane_id);
                }
                browser.into_any_element()
            }
            #[cfg(not(target_os = "ios"))]
            PaneKindSnapshot::Browser(descriptor) => super::unsupported_pane(
                "Browser",
                IconName::Globe,
                "Embedded Chromium needs the desktop app. Open this page in a browser tab.",
                descriptor.tabs.get(descriptor.active_tab).cloned(),
                Corners::all(radius),
                cx,
            ),
            PaneKindSnapshot::Editor(_) => super::unsupported_pane(
                "Editor",
                IconName::File,
                "The daemon does not share editor file contents with browser clients yet.",
                None,
                Corners::all(radius),
                cx,
            ),
            PaneKindSnapshot::Picker => {
                let picker = self.picker_entity(pane_id, cx);
                picker.update(cx, |picker, cx| {
                    picker.set_agent_enabled(self.preferences.agent_enabled);
                    picker.set_corner_radii(Corners::all(radius), cx);
                });
                if can_focus && self.focused_pane != Some(pane_id) {
                    picker.read(cx).focus_handle(cx).focus(window, cx);
                    self.focused_pane = Some(pane_id);
                }
                picker.into_any_element()
            }
        };
        let writable = {
            let connection = self.connection.read(cx);
            connection.connected && !connection.core.attached_read_only()
        };
        let actions = writable.then(|| {
            pane_header_icon_button(("compact-close-pane", pane_id.0), IconName::Xmark, true, cx)
                .hit_slop(10.0, 10.0)
                .tooltip("Close pane")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.command("kill-pane", vec!["-t".into(), pane_id.to_string()], cx);
                }))
                .into_any_element()
        });
        let header = matches!(pane.kind, PaneKindSnapshot::Terminal)
            .then(|| compact_pane_header(kind_icon(&pane.kind), slot.title.clone(), actions, cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .pt(top_inset)
            .bg(background)
            .rounded(radius)
            .children(header)
            .child(div().flex_1().min_h_0().min_w_0().child(content))
            .into_any_element()
    }

    fn compact_can_focus(&self, window: &Window, cx: &App) -> bool {
        let core = &self.connection.read(cx).core;
        self.prompt.is_none()
            && !self.slideover
            && !self.sidebar_focus.is_focused(window)
            && core.choose_tree().is_none()
            && core.choose_buffer().is_none()
            && core.command_prompt().is_none()
            && core.menu().is_none()
            && core.confirm().is_none()
            && core.popup().is_none()
            && core.command_output().is_none()
    }

    fn focus_current_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(pane) = self.active_window(cx).map(|window| window.active_pane)
            && let Some(terminal) = self.terminals.get(&pane)
        {
            terminal.read(cx).focus_handle(cx).focus(window, cx);
            self.focused_pane = Some(pane);
        }
    }

    #[cfg(target_os = "ios")]
    pub(super) fn compact_visible_panes(&self, cx: &App) -> BTreeSet<PaneId> {
        if self.compact.pager.is_moving() {
            return BTreeSet::new();
        }
        self.active_window(cx)
            .map(|window| BTreeSet::from([window.active_pane]))
            .unwrap_or_default()
    }

    pub(super) fn compact_shell(
        &mut self,
        terminal_overlays: Vec<AnyElement>,
        overlays: Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        self.prune_pane_entities(cx);
        let model = self.status_model(cx);
        let (pages, current) = pages(&model, &self.unseen_agents);
        let current_pane = pages.get(current).map(|page| page.pane);
        if self.compact.last_current.is_some() && current_pane != self.compact.last_current {
            self.compact.zoom_intent = current_pane;
            self.compact.previous = self.compact.last_current;
        }
        self.compact.last_current = current_pane;
        self.reconcile_compact_zoom(cx);
        let width = f32::from(window.viewport_size().width);
        self.compact.width = width;
        self.compact.pager.sync(current, pages.len());
        let animating = self.compact.pager.tick(Instant::now());
        let event = self.compact.pager.take_event();
        self.commit_page(event, cx);
        if animating {
            window.request_animation_frame();
        } else if self.terminal_resize_suppressed.get()
            && self.split_drag.is_none()
            && self.compact.pinch.is_none()
            && !self.compact.keyboard_moving
        {
            self.terminal_resize_suppressed.set(false);
        }
        let motion = self.compact.pager.motion(width);
        let layout = self.compact.pager.layout(width, PAGE_GAP);
        let visible = window.fully_visible_bounds();
        let top_inset = visible.top();
        let keyboard = keyboard_visible(window);
        let safe_bottom = (window.viewport_size().height - visible.bottom()).max(px(0.0));
        let overlap = window.viewport_size().height - window.visual_viewport_bounds().bottom();
        if safe_bottom > overlap || overlap <= px(0.0) {
            self.compact.safe_bottom = safe_bottom;
        }
        let shown = self.compact.pager.index();
        let mut strip = Vec::new();
        for (index, x) in layout.pages {
            let Some(page) = pages.get(index).cloned() else {
                continue;
            };
            let element = self.compact_page(&page, index == shown, motion, top_inset, window, cx);
            strip.push(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(x))
                    .w(px(width))
                    .child(element)
                    .into_any_element(),
            );
        }
        let pager_area = div()
            .id("compact-pager")
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                this.compact_scroll(event, window, cx);
            }))
            .on_pinch(cx.listener(|this, event: &PinchEvent, window, cx| {
                if event.phase == TouchPhase::Started {
                    this.compact_pinch(event, window, cx);
                }
            }))
            .capture_pinch(cx.listener(|this, event: &PinchEvent, window, cx| {
                if event.phase != TouchPhase::Started {
                    this.compact_pinch(event, window, cx);
                }
            }))
            .children(strip)
            .child(top_shade(top_inset, cx))
            .children(terminal_overlays)
            .when(
                self.compact.pinch.is_some()
                    || self
                        .compact
                        .scale_hud
                        .is_some_and(|until| Instant::now() < until),
                |area| {
                    area.child(compact_hud(
                        format!("{:.0}%", self.preferences.terminal_font_scale * 100.0),
                        cx,
                    ))
                },
            );
        if !keyboard && let Some((keys, _)) = &self.compact.tool_keys {
            keys.clone().close(cx);
        }
        let bottom = if keyboard {
            let bar = rems_from_px(COMPACT_BAR_HEIGHT).to_pixels(window.rem_size()) + px(1.0);
            let lift = key_row_lift(self.compact.safe_bottom, bar, overlap);
            self.compact_key_area(lift, window, cx)
        } else {
            let page = pages.get(shown);
            let groups = dot_groups(&pages, shown);
            let title = compact_bar_title(
                "compact-title",
                page.map_or(IconName::SquareTerminal, |page| kind_icon(&page.kind)),
                page.map(|page| page.title.clone()).unwrap_or_default(),
                page_dots(&groups, cx),
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| this.tmux_command("choose-tree -w", cx)));
            let tree = compact_bar_button("compact-tree", IconName::PanelsTopLeft)
                .tooltip("Workspace")
                .on_click(cx.listener(|this, _, window, cx| this.focus_sidebar(window, cx)));
            let keys = compact_bar_button("compact-keyboard", IconName::Keyboard)
                .tooltip("Keyboard")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.focus_current_pane(window, cx);
                    window.request_virtual_keyboard();
                    cx.notify();
                }));
            div()
                .flex_none()
                .w_full()
                .pb(safe_bottom)
                .bg(cx.theme().background)
                .border_t_1()
                .border_color(cx.theme().border())
                .child(compact_bar(tree, title, keys, cx))
                .into_any_element()
        };
        let mut sheets = Vec::new();
        if self.slideover {
            sheets.push(self.compact_tree_sheet(safe_bottom, window, cx));
        }
        if self.compact.bindings {
            let inset = if keyboard { px(0.0) } else { safe_bottom };
            sheets.push(self.compact_bindings_sheet(inset, cx));
        }
        div()
            .id("compact-shell")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .child(pager_area)
            .child(bottom)
            .children(sheets)
            .child(zz_ui::touch::touch_scale(
                div()
                    .absolute()
                    .top(top_inset)
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .children(overlays),
                cx,
            ))
    }

    fn compact_key_area(
        &mut self,
        bottom: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let keys = self.tool_keys(window, cx);
        let armed = self.connection.read(cx).core.prefix_armed();
        keys.prefix.update(cx, |key, cx| key.set_armed(armed, cx));
        let row = KeyRow::new(keys.row())
            .on_key(|keystroke, window, cx| dispatch_key(keystroke.clone(), window, cx))
            .on_hide(|window, _| window.dismiss_virtual_keyboard());
        div()
            .relative()
            .flex_none()
            .w_full()
            .h(px(KEY_ROW_HEIGHT) + bottom)
            .pb(bottom)
            .bg(cx.theme().background)
            .border_t_1()
            .border_color(cx.theme().border())
            .child(row)
            .into_any_element()
    }

    fn compact_bindings_sheet(
        &mut self,
        bottom_inset: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let core = &self.connection.read(cx).core;
        let prefix = prefix_spelling(core);
        let rows = binding_rows(core, &prefix);
        let cap = tmux_keystroke(&prefix).map(|key| Kbd::new(key).lowercase().into_any_element());
        let pick = cx.weak_entity();
        let dismiss = cx.weak_entity();
        bottom_sheet(
            "compact-bindings-sheet",
            "All bindings",
            cap,
            WhichKeyList::new(rows).on_pick(move |id, _, cx| {
                let _ = pick.update(cx, |this, cx| {
                    this.compact.bindings = false;
                    this.send_binding(id, cx);
                    cx.notify();
                });
            }),
            bottom_inset,
            move |_, cx| {
                let _ = dismiss.update(cx, |this, cx| {
                    this.compact.bindings = false;
                    cx.notify();
                });
            },
        )
        .into_any_element()
    }

    fn compact_tree_sheet(
        &mut self,
        bottom_inset: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        sidebar::reconcile(self, window, cx);
        let core = &self.connection.read(cx).core;
        let tree = sidebar::session_tree(
            core.snapshot(),
            core.attached_session(),
            sidebar::Runtime {
                connection: self.connection.clone(),
                focus: self.sidebar_focus.clone(),
                focused: self.sidebar_focus.is_focused(window),
                view: cx.entity(),
                selected: (!self.sidebar_pointer_selection)
                    .then_some(self.sidebar_selection)
                    .flatten(),
                unseen_agents: self.unseen_agents.clone(),
            },
            &self.collapsed_tree,
            &self.sidebar_scroll,
            cx,
        );
        let height = window.viewport_size().height * 0.62;
        let settings = compact_bar_button("compact-sheet-settings", IconName::Settings)
            .tooltip("Settings")
            .on_click(cx.listener(|this, _, _, cx| {
                this.slideover = false;
                this.settings = Some(zz_ui::settings::SettingsSection::Appearance);
                this.focused_pane = None;
                cx.notify();
            }))
            .into_any_element();
        let view = cx.weak_entity();
        bottom_sheet(
            "compact-tree-sheet",
            "Workspace",
            [settings],
            div()
                .h(height)
                .w_full()
                .track_focus(&self.sidebar_focus)
                .child(zz_ui::touch::touch_scale(tree, cx)),
            bottom_inset,
            move |window, cx| {
                let _ = view.update(cx, |this, cx| this.release_sidebar_focus(window, cx));
            },
        )
        .content_scroll(self.sidebar_scroll.0.borrow().base_handle.clone())
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_spellings_become_gpui_keystrokes() {
        let control = tmux_keystroke("C-b").expect("prefix");
        assert!(control.modifiers.control);
        assert_eq!(control.key, "b");
        let percent = tmux_keystroke("%").expect("percent");
        assert_eq!(percent.key, "%");
        assert_eq!(percent.key_char.as_deref(), Some("%"));
        let upper = tmux_keystroke("L").expect("upper");
        assert!(upper.modifiers.shift);
        assert_eq!(upper.key, "l");
        assert_eq!(
            tmux_keystroke("Escape").map(|key| key.key),
            Some("escape".into())
        );
        assert_eq!(
            tmux_keystroke("M-Up").map(|key| (key.key, key.modifiers.alt)),
            Some(("up".into(), true))
        );
        assert!(tmux_keystroke("F13-nope").is_none());
    }

    #[test]
    fn pinch_scales_terminal_text_in_five_percent_steps_within_the_setting_range() {
        assert_eq!(pinch_font_scale(1.0, 1.0), 1.0);
        assert_eq!(pinch_font_scale(1.0, 1.02), 1.0);
        assert_eq!(pinch_font_scale(1.0, 1.5), 1.5);
        assert_eq!(pinch_font_scale(1.2, 1.12), 1.35);
        assert_eq!(pinch_font_scale(1.0, 0.8), 0.8);
        assert_eq!(pinch_font_scale(2.5, 2.0), 3.0);
        assert_eq!(pinch_font_scale(0.6, 0.1), 0.5);
        assert_eq!(pinch_font_scale(1.1, f32::NAN), 1.1);
    }

    #[test]
    fn the_key_row_rests_on_the_bar_line_until_the_keyboard_passes_it() {
        let (safe, bar) = (px(34.0), px(53.0));
        let top =
            |overlap: f32| px(KEY_ROW_HEIGHT) + key_row_lift(safe, bar, px(overlap)) + px(overlap);
        assert_eq!(top(0.0), safe + bar);
        assert_eq!(top(40.0), safe + bar);
        assert_eq!(top(300.0), px(KEY_ROW_HEIGHT + 300.0));
    }

    #[test]
    fn dots_group_by_window_and_flag_attention_off_page() {
        let page = |pane: u32, window: u32, attention: bool| Page {
            pane: PaneId(pane.into()),
            window: WindowId(window.into()),
            kind: PaneKindSnapshot::Terminal,
            title: SharedString::default(),
            attention,
        };
        let pages = [
            page(0, 1, false),
            page(1, 1, true),
            page(2, 2, true),
            page(3, 3, false),
        ];
        let groups = dot_groups(&pages, 2);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].len(), 2);
        assert!(groups[0][1].attention);
        assert!(groups[1][0].active);
        assert!(!groups[1][0].attention);
    }
}
