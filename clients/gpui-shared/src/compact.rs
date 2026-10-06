use std::{collections::BTreeSet, sync::Arc};

use gpui::{
    AnyElement, App, Context, Corners, Entity, Focusable as _, IntoElement, Keystroke,
    ParentElement as _, Pixels, ScrollDelta, ScrollWheelEvent, SharedString, Stateful, Styled as _,
    Subscription, Window, div, prelude::*, px,
};
use zz_client::StatusBarModel;
use zz_protocol::{PaneId, PaneKindSnapshot, WindowId};
use zz_ui::{
    ActiveTheme as _, IconName,
    compact::{
        ArrowPad, ArrowPadEvent, Instant, KEY_ROW_HEIGHT, KeyRow, PageDot, Pager, PagerEvent,
        WhichKeyList, bottom_sheet, compact_bar, compact_bar_button, compact_bar_title,
        compact_pane_header, page_dots, top_shade,
    },
    pane::pane_header_icon_button,
    which_key::{WhichKeyCap, WhichKeyHeader, WhichKeyRow},
};

use super::{AppShell, sidebar};

const PAGE_GAP: f32 = 12.0;
const CARD_RADIUS: f32 = 22.0;
const WHICH_KEY_INSET: f32 = 10.0;
const WHICH_KEY_GAP: f32 = 8.0;

#[derive(Default)]
pub(super) struct CompactState {
    pager: Pager,
    width: f32,
    arrow_pad: Option<(Entity<ArrowPad>, Subscription)>,
    preview: Option<bool>,
    zoom_intent: Option<PaneId>,
    compact_keyboard: Option<bool>,
    last_current: Option<PaneId>,
}

impl CompactState {
    pub(super) fn reset_attachment(&mut self) {
        self.preview = None;
        self.zoom_intent = None;
        self.last_current = None;
    }
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

fn keyboard_visible(window: &Window) -> bool {
    window.visual_viewport_bounds().size.height + px(1.0) < window.viewport_size().height
}

fn which_key(core: &zz_client::ClientCore) -> Option<(WhichKeyHeader, Arc<[WhichKeyRow]>)> {
    let prefix = core
        .mux_options()
        .get(zz_protocol::MuxOptionKey::Prefix)
        .filter(|option| !option.value.eq_ignore_ascii_case("none"))
        .map_or_else(
            || "C-b".to_owned(),
            |option| zz_protocol::canonical_key(&option.value),
        );
    let cap = |keys: &[String], yours: bool| WhichKeyCap {
        keys: keys
            .iter()
            .map(|key| tmux_keystroke(key))
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default(),
        raw: keys.join(" ").into(),
        yours,
    };
    let rows: Arc<[WhichKeyRow]> = zz_client::which_key::rows(core.key_tables(), "prefix", &prefix)
        .into_iter()
        .filter(|row| row.core)
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
        .collect();
    if rows.is_empty() {
        return None;
    }
    let more = core
        .prefix_bindings()
        .iter()
        .find(|binding| zz_client::which_key::opens_all_keys(&binding.commands))
        .map(|binding| cap(std::slice::from_ref(&binding.key), false));
    Some((
        WhichKeyHeader {
            table: "prefix".into(),
            prefix: tmux_keystroke(&prefix),
            prefix_raw: prefix.into(),
            more,
        },
        rows,
    ))
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

    fn arrow_pad(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<ArrowPad> {
        if let Some((pad, _)) = &self.compact.arrow_pad {
            return pad.clone();
        }
        let pad = cx.new(ArrowPad::new);
        let subscription =
            cx.subscribe_in(&pad, window, |_, _, event: &ArrowPadEvent, window, cx| {
                let ArrowPadEvent::Arrow(direction) = event;
                if let Ok(keystroke) = Keystroke::parse(direction.key()) {
                    let keystroke = zz_ui::compact::StickyModifiers::apply(&keystroke, cx);
                    dispatch_key(keystroke, window, cx);
                }
            });
        self.compact.arrow_pad = Some((pad.clone(), subscription));
        pad
    }

    fn compact_scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
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
        self.terminal_resize_suppressed
            .set(self.compact.pager.is_moving());
        self.commit_page(response.event, cx);
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
        mut overlays: Vec<AnyElement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<gpui::Div> {
        self.prune_pane_entities(cx);
        let model = self.status_model(cx);
        let (pages, current) = pages(&model, &self.unseen_agents);
        let current_pane = pages.get(current).map(|page| page.pane);
        if self.compact.last_current.is_some() && current_pane != self.compact.last_current {
            self.compact.zoom_intent = current_pane;
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
        } else if self.terminal_resize_suppressed.get() && self.split_drag.is_none() {
            self.terminal_resize_suppressed.set(false);
        }
        let motion = self.compact.pager.motion(width);
        let layout = self.compact.pager.layout(width, PAGE_GAP);
        let visible = window.fully_visible_bounds();
        let top_inset = visible.top();
        let keyboard = keyboard_visible(window);
        let safe_bottom = (window.viewport_size().height - visible.bottom()).max(px(0.0));
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
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                this.compact_scroll(event, cx);
            }))
            .children(strip)
            .child(top_shade(top_inset, cx))
            .children(terminal_overlays);
        let bottom = if keyboard {
            self.compact_key_area(window, cx)
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
        if self.slideover {
            overlays.insert(0, self.compact_tree_sheet(safe_bottom, window, cx));
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
            .children(overlays)
    }

    fn compact_key_area(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let pad = self.arrow_pad(window, cx);
        let core = &self.connection.read(cx).core;
        let armed = core.prefix_armed();
        let which = armed.then(|| which_key(core)).flatten();
        let prefix = core
            .mux_options()
            .get(zz_protocol::MuxOptionKey::Prefix)
            .filter(|option| !option.value.eq_ignore_ascii_case("none"))
            .map_or_else(
                || "C-b".to_owned(),
                |option| zz_protocol::canonical_key(&option.value),
            );
        let row = KeyRow::new(pad)
            .prefix_label("prefix")
            .prefix_armed(armed)
            .on_key(|keystroke, window, cx| dispatch_key(keystroke.clone(), window, cx))
            .on_prefix(move |window, cx| {
                let key = if armed { Some("Escape") } else { None };
                if let Some(keystroke) = tmux_keystroke(key.unwrap_or(&prefix)) {
                    dispatch_key(keystroke, window, cx);
                }
            })
            .on_hide(|window, _| window.dismiss_virtual_keyboard());
        let room = window.visual_viewport_bounds().size.height
            - window.fully_visible_bounds().top()
            - px(KEY_ROW_HEIGHT + 2.0 * WHICH_KEY_GAP);
        let list = which.map(|(header, rows)| {
            div()
                .absolute()
                .left(px(WHICH_KEY_INSET))
                .right(px(WHICH_KEY_INSET))
                .bottom(px(KEY_ROW_HEIGHT + WHICH_KEY_GAP))
                .max_h(room)
                .flex()
                .flex_col()
                .child(
                    WhichKeyList::new(header, rows)
                        .on_pick(|id, window, cx| {
                            if let Some(keystroke) = tmux_keystroke(id) {
                                dispatch_key(keystroke, window, cx);
                            }
                        })
                        .on_more(|window, cx| {
                            if let Some(keystroke) = tmux_keystroke("?") {
                                dispatch_key(keystroke, window, cx);
                            }
                        }),
                )
        });
        div()
            .relative()
            .flex_none()
            .w_full()
            .h(px(KEY_ROW_HEIGHT))
            .bg(cx.theme().background)
            .border_t_1()
            .border_color(cx.theme().border())
            .child(row)
            .children(list)
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
                .child(tree),
            bottom_inset,
            move |window, cx| {
                let _ = view.update(cx, |this, cx| this.release_sidebar_focus(window, cx));
            },
            cx,
        )
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
