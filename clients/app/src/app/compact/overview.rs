use std::{ops::Range, rc::Rc};

use zz_client::{StatusBarModel, navigation::session_label, window_summary::summarize_window};
use zz_gpui::{
    AnyElement, App, Bounds, Context, Entity, Font, Hsla, ParentElement as _, Pixels, ScrollDelta,
    ScrollWheelEvent, StyledText, TextRun, Window, canvas, div, font, linear_color_stop,
    linear_gradient, point, prelude::*, px, size,
};
use zz_protocol::{PaneId, PaneKindSnapshot, SessionId, WindowId};
use zz_terminal::{Glyph, LocalColors, TerminalAppearance, TerminalPalette, TerminalViewport};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Disableable as _, Edge, Icon, IconName, PaintState,
    StyledExt as _,
    button::{Button, ButtonVariants as _},
    compact::{
        COMPACT_NAV_BAR_HEIGHT, COMPACT_PANE_HEADER_HEIGHT, HostState, Instant, OVERVIEW_CARD_GAP,
        OVERVIEW_CARD_HEIGHT, OVERVIEW_CARD_WIDTH, PageDot, PillLabel, PillPlacement, bottom_sheet,
        claim_back_swipe, close_sheet, compact_bar_button, compact_floating_bar,
        compact_floating_group, compact_floating_pill, overview_card, overview_choice_row,
        overview_count, overview_host_empty, overview_host_row, overview_section_heading,
        overview_session_heading, page_dots, yield_back_swipe,
    },
    rems_from_px, roles,
    terminal::terminal_background,
};

use zz_ui::compact::sheet_close;

use super::{AppShell, Page, kind_icon, pages};
use crate::{connection::Connection, terminal::phone_colors};
#[cfg(any(target_os = "ios", target_os = "android"))]
use zz_ui::{compact::sheet_row, input::Input};

const PREVIEW_TEXT: f32 = 4.0;
const PREVIEW_LINE: f32 = 4.8;
const PREVIEW_ROWS: u16 = 40;
const PREVIEW_COLUMNS: usize = 56;
const OVERVIEW_FADE: f32 = 1.5;
const LIVE_FADE: f32 = 3.0;
const CARD_FADE: f32 = 0.15;
pub(super) const HOSTS_SHEET: &str = "overview-hosts-sheet";
pub(super) const NEW_SHEET: &str = "overview-new-sheet";
#[cfg(any(target_os = "ios", target_os = "android"))]
pub(super) const ADD_HOST_SHEET: &str = "overview-add-host-sheet";
const HOST_GAP: f32 = 12.0;
const EDGE_FADE: f32 = 24.0;
const TEXT_STEP: f32 = 4.0;

fn quantized(size: Pixels) -> Pixels {
    px((f32::from(size) * TEXT_STEP).round() / TEXT_STEP)
}

fn host_palette(core: &zz_client::ClientCore) -> TerminalPalette {
    core.appearance().map_or_else(
        || TerminalAppearance::default().palette,
        |appearance| appearance.palette,
    )
}

fn terminal_preview(
    viewport: &TerminalViewport,
    local: Option<LocalColors>,
    base: &Font,
    rows: Range<u16>,
    columns: usize,
) -> StyledText {
    let fallback = terminal_background(
        local.map_or(viewport.foreground, |local| local.foreground()),
        1.0,
    );
    let mut text = String::new();
    let mut runs: Vec<TextRun> = Vec::new();
    let mut push = |text: &mut String, start: usize, color: Hsla| {
        let len = text.len() - start;
        match runs.last_mut() {
            Some(run) if run.color == color => run.len += len,
            _ => runs.push(TextRun {
                len,
                font: base.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            }),
        }
    };
    let first = rows.start;
    for row in rows {
        if row > first {
            let start = text.len();
            text.push('\n');
            push(&mut text, start, fallback);
        }
        let Some(cells) = viewport.row(row) else {
            continue;
        };
        let cells = &cells[..cells.len().min(columns)];
        let end = cells
            .iter()
            .rposition(|cell| match viewport.glyph(*cell) {
                Glyph::Empty => false,
                Glyph::Scalar(character) => !character.is_whitespace(),
                Glyph::Grapheme(_) => true,
            })
            .map_or(0, |last| last + 1);
        for cell in &cells[..end] {
            let start = text.len();
            viewport.push_glyph(*cell, &mut text);
            if text.len() == start {
                text.push(' ');
            }
            let color = viewport.style(*cell).map_or(fallback, |style| {
                let style = local.map_or(style, |local| local.recolor(style));
                terminal_background(style.foreground(), 1.0)
            });
            push(&mut text, start, color);
        }
    }
    StyledText::new(text).with_runs(runs)
}

impl AppShell {
    pub(in super::super) fn open_overview(&mut self, cx: &mut Context<Self>) {
        if !self.compact.overview {
            let hosts = self.host_pages(cx);
            let attached = hosts
                .iter()
                .position(|host| host.connection.as_ref() == Some(&self.connection))
                .unwrap_or(0);
            self.compact.host_pager.sync(attached, hosts.len());
            self.compact.host_pager.go_to(attached, Instant::now());
            self.compact.host_pager.finish();
            self.compact.overview = true;
            self.compact.overview_reveal = true;
            self.compact.landing = None;
            self.compact.lift.animate(true, Instant::now());
        }
        cx.notify();
    }

    pub(in super::super) fn close_overview(&mut self, cx: &mut Context<Self>) {
        self.compact.overview = false;
        self.compact.close_sheets();
        self.compact.landing = None;
        self.compact.lift.snap(false);
        self.focused_pane = None;
        cx.notify();
    }

    pub(in super::super) fn land_overview(&mut self, pane: Option<PaneId>, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.compact.overview = false;
        self.compact.close_sheets();
        self.compact.landing = pane.map(|pane| (pane, now));
        self.compact.lift.animate(false, now);
        self.focused_pane = None;
        cx.notify();
    }

    pub(super) fn compact_overview(
        &mut self,
        model: &StatusBarModel,
        top_inset: Pixels,
        placement: &PillPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hosts = self.host_pages(cx);
        let width = f32::from(window.viewport_size().width);
        let shown = self.compact.host_pager.index();
        self.compact.host_pager.sync(shown, hosts.len());
        if cx.reduce_motion() {
            self.compact.host_pager.finish();
        }
        if self.compact.host_pager.tick(Instant::now()) {
            window.request_animation_frame();
        }
        self.compact.host_pager.take_event();
        let index = self.compact.host_pager.index();
        let layout = self.compact.host_pager.layout(width, HOST_GAP);
        let mut reveal = None;
        let mut windows = Vec::new();
        let mut panes = Vec::new();
        let mut strip = Vec::new();
        for (page, x) in layout.pages {
            let Some(host) = hosts.get(page) else {
                continue;
            };
            let body = self.host_body(
                page,
                host,
                model,
                top_inset,
                placement,
                &mut reveal,
                &mut windows,
                &mut panes,
                cx,
            );
            strip.push(
                div()
                    .id(("overview-host", page))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(x))
                    .w(px(width))
                    .child(body)
                    .into_any_element(),
            );
        }
        self.compact
            .overview_rows
            .retain(|row, _| windows.contains(row));
        self.compact
            .cards
            .borrow_mut()
            .retain(|pane, _| panes.contains(pane));
        if self.compact.overview_reveal {
            match reveal {
                Some((page, section, window, card)) => {
                    self.reveal_current(page, section, window, card, cx);
                }
                None => self.compact.overview_reveal = false,
            }
        }
        let zone = placement.zone();
        let scrim = cx.theme().background.opaque();
        let top = top_inset + px(EDGE_FADE);
        div()
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .child(div().absolute().inset_0().overflow_hidden().children(strip))
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .h(top)
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(scrim, top_inset / top),
                        linear_color_stop(scrim.alpha(0.0), 1.0),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .h(zone)
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(scrim.alpha(0.0), 0.0),
                        linear_color_stop(scrim, 0.55),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .child(self.overview_bar(&hosts, index, placement, window, cx)),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn host_body(
        &mut self,
        page: usize,
        host: &HostPage,
        model: &StatusBarModel,
        top_inset: Pixels,
        placement: &PillPlacement,
        reveal: &mut Option<(usize, usize, WindowId, usize)>,
        windows: &mut Vec<(usize, WindowId)>,
        panes: &mut Vec<PaneId>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let attached = host
            .connection
            .as_ref()
            .is_some_and(|connection| *connection == self.connection);
        if !attached && !host.connected(cx) {
            return self.empty_host(host, cx);
        }
        let connection = host
            .connection
            .clone()
            .unwrap_or_else(|| self.connection.clone());
        let sections = self.host_sections(
            page,
            host,
            &connection,
            attached.then_some(model),
            reveal,
            windows,
            panes,
            cx,
        );
        if sections.is_empty() {
            return self.empty_host(host, cx);
        }
        let scroll = self.compact.host_scrolls.entry(page).or_default().clone();
        div()
            .id(("overview-scroll", page))
            .size_full()
            .overflow_y_scroll()
            .restrict_scroll_to_axis()
            .track_scroll(&scroll)
            .pt(top_inset)
            .pb(placement.zone() + px(16.0))
            .children(sections)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn host_sections(
        &mut self,
        page: usize,
        host: &HostPage,
        connection: &Entity<Connection>,
        attached_model: Option<&StatusBarModel>,
        reveal: &mut Option<(usize, usize, WindowId, usize)>,
        drawn_windows: &mut Vec<(usize, WindowId)>,
        drawn_panes: &mut Vec<PaneId>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let settings = self.preferences.status_bar_settings();
        let attached_session = attached_model.and(connection.read(cx).core.attached_session());
        let sessions: Vec<(SessionId, String, StatusBarModel, Vec<WindowTitle>)> = {
            let core = &connection.read(cx).core;
            core.snapshot()
                .sessions
                .iter()
                .map(|session| {
                    let model = match attached_model {
                        Some(model) if attached_session == Some(session.id) => model.clone(),
                        _ => StatusBarModel::from_snapshot(
                            core.snapshot(),
                            Some(session.id),
                            None,
                            settings,
                        ),
                    };
                    let titles = session
                        .windows
                        .iter()
                        .map(|window| WindowTitle {
                            id: window.id,
                            title: summarize_window(window).title,
                            folder: window
                                .panes
                                .get(&window.active_pane)
                                .map(|pane| pane.summary.cwd.clone())
                                .filter(|cwd| !cwd.is_empty()),
                        })
                        .collect();
                    (session.id, session_label(session).text, model, titles)
                })
                .collect()
        };
        let mut sections = Vec::new();
        for (session, name, model, titles) in sessions {
            let live = attached_session == Some(session);
            let (pages, current) = pages(&model, &self.unseen_agents);
            let shown = live
                .then(|| pages.get(current).map(|page| page.window))
                .flatten();
            let current = live
                .then(|| pages.get(current).map(|page| page.pane))
                .flatten();
            sections.push(overview_session_heading(&name, cx).into_any_element());
            for window in &model.windows {
                drawn_windows.push((page, window.id));
                let target = if live {
                    CardTarget::Here
                } else if attached_model.is_some() {
                    CardTarget::Session(session)
                } else {
                    CardTarget::Host(host.endpoint.clone().unwrap_or_default(), session)
                };
                let row: Vec<&Page> = pages
                    .iter()
                    .filter(|page| page.window == window.id)
                    .collect();
                if shown == Some(window.id)
                    && let Some(card) = row.iter().position(|page| Some(page.pane) == current)
                {
                    *reveal = Some((page, sections.len(), window.id, card));
                }
                let cards: Vec<AnyElement> = row
                    .iter()
                    .map(|page| {
                        drawn_panes.push(page.pane);
                        self.overview_card(
                            page,
                            current == Some(page.pane),
                            connection,
                            target.clone(),
                            cx,
                        )
                    })
                    .collect();
                let title = titles.iter().find(|title| title.id == window.id);
                sections.push(self.overview_section(
                    page,
                    window.id,
                    window.index,
                    title.map_or(window.label.as_str(), |title| title.title.as_str()),
                    title.and_then(|title| title.folder.as_deref()),
                    cards,
                    cx,
                ));
            }
        }
        sections
    }

    fn empty_host(&self, host: &HostPage, cx: &mut Context<Self>) -> AnyElement {
        let (connected, busy, status) =
            host.connection
                .as_ref()
                .map_or((false, false, String::new()), |connection| {
                    let connection = connection.read(cx);
                    (
                        connection.connected,
                        connection.busy_or_connecting(),
                        connection.status.clone(),
                    )
                });
        let endpoint = host.endpoint.clone();
        let (detail, action): (String, Option<AnyElement>) = if connected {
            (
                "No sessions yet.".into(),
                Some(
                    Button::new("overview-empty-new-session")
                        .label("New session")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.create(NewThing::Session, window, cx);
                        }))
                        .into_any_element(),
                ),
            )
        } else if busy {
            (
                "Connecting…".into(),
                endpoint.map(|endpoint| {
                    Button::new("overview-empty-cancel")
                        .label("Cancel")
                        .ghost()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            #[cfg(any(target_os = "ios", target_os = "android"))]
                            this.disconnect_host(&endpoint, window, cx);
                            let _ = (this, &endpoint, window, cx);
                        }))
                        .into_any_element()
                }),
            )
        } else {
            let detail = Some(status)
                .filter(|status| !status.is_empty() && status != "Not connected")
                .unwrap_or_else(|| {
                    format!(
                        "{} is not connected. Your other hosts stay connected while it signs in.",
                        host.endpoint.as_deref().unwrap_or(&host.title)
                    )
                });
            (
                detail,
                endpoint.map(|endpoint| {
                    Button::new("overview-empty-connect")
                        .label("Connect")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            #[cfg(any(target_os = "ios", target_os = "android"))]
                            this.connect_host(endpoint.clone(), window, cx);
                            let _ = (this, &endpoint, window, cx);
                        }))
                        .into_any_element()
                }),
            )
        };
        overview_host_empty(host.title.clone(), detail, action, cx).into_any_element()
    }

    pub(super) fn overview_lift(
        &mut self,
        model: &StatusBarModel,
        stage: Bounds<Pixels>,
        top_inset: Pixels,
        placement: &PillPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> zz_gpui::Div {
        let progress = self.compact.lift.progress();
        let (pages, current) = pages(model, &self.unseen_agents);
        let target = self
            .compact
            .landing
            .map(|(pane, _)| pane)
            .or_else(|| pages.get(current).map(|page| page.pane));
        let overview = self.compact_overview(model, top_inset, placement, window, cx);
        let card = pages
            .iter()
            .find(|page| Some(page.pane) == target)
            .map(|page| self.lift_card(page, progress, stage, top_inset, window, cx));
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .opacity((progress * OVERVIEW_FADE).min(1.0))
                    .child(overview),
            )
            .child(
                div()
                    .id("overview-lift-block")
                    .absolute()
                    .inset_0()
                    .occlude(),
            )
            .children(card)
    }

    fn lift_card(
        &mut self,
        page: &Page,
        progress: f32,
        stage: Bounds<Pixels>,
        top_inset: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let card = self
            .compact
            .cards
            .borrow()
            .get(&page.pane)
            .copied()
            .unwrap_or_else(|| {
                Bounds::new(
                    point(px(16.0), top_inset + px(COMPACT_NAV_BAR_HEIGHT)),
                    size(px(OVERVIEW_CARD_WIDTH), px(OVERVIEW_CARD_HEIGHT)),
                )
            });
        let settled = progress.min(1.0);
        let rest = f32::from(card.size.width) / f32::from(stage.size.width);
        let scale = 1.0 + (rest - 1.0) * progress;
        let tall = f32::from(stage.size.height);
        let height = (tall + (f32::from(card.size.height) / rest - tall) * settled) * scale;
        let toward = |from: Pixels, to: Pixels| from + (to - from) * progress;
        let theme = cx.theme();
        let frame = theme.layer(roles::PHONE_CARD, PaintState::Rest);
        let card_radius = frame.radius;
        let muted = theme.foreground.muted();
        let background = theme.background.opaque().blend(Hsla {
            a: frame.fill.a * settled,
            ..frame.fill
        });
        let mut layers: Vec<AnyElement> = Vec::new();
        let text = self.compact.text.clone();
        let terminal = {
            let core = &self.connection.read(cx).core;
            match (&page.kind, core.viewport(page.pane), text) {
                (PaneKindSnapshot::Terminal, Some(viewport), Some(text)) => {
                    let first = viewport.rows.saturating_sub(text.rows);
                    Some((
                        terminal_preview(
                            viewport,
                            phone_colors(viewport, &host_palette(core), cx),
                            &text.font,
                            first..viewport.rows,
                            usize::from(viewport.columns),
                        ),
                        text,
                    ))
                }
                _ => None,
            }
        };
        if let Some((preview, text)) = terminal {
            let rem = |value: f32| rems_from_px(value).to_pixels(window.rem_size()) * scale;
            layers.push(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(top_inset * scale)
                    .h(rem(COMPACT_PANE_HEADER_HEIGHT))
                    .flex()
                    .items_center()
                    .gap(rem(8.0))
                    .pl(rem(12.0))
                    .whitespace_nowrap()
                    .font_family(theme.font_family.clone())
                    .text_size(quantized(rem(15.0)))
                    .line_height(rem(20.0))
                    .text_color(muted)
                    .child(
                        div()
                            .opacity(0.8)
                            .child(Icon::new(kind_icon(&page.kind)).size(rem(16.0))),
                    )
                    .child(page.title.clone())
                    .into_any_element(),
            );
            layers.push(
                div()
                    .absolute()
                    .left((text.origin.x - stage.origin.x) * scale)
                    .top((text.origin.y - stage.origin.y) * scale)
                    .whitespace_nowrap()
                    .font(text.font)
                    .text_size(quantized(text.font_size * scale))
                    .line_height(text.line_height * scale)
                    .child(preview)
                    .into_any_element(),
            );
        } else {
            let live = (1.0 - settled * LIVE_FADE).max(0.0);
            if live > 0.0 {
                let element =
                    self.compact_page(page, false, 0.0, top_inset, px(0.0), None, window, cx);
                layers.push(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w(stage.size.width)
                        .h(stage.size.height)
                        .opacity(live)
                        .child(element)
                        .into_any_element(),
                );
            }
            layers.push(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(settled)
                    .text_color(muted)
                    .child(Icon::new(kind_icon(&page.kind)).size(px(20.0)))
                    .into_any_element(),
            );
        }
        div()
            .absolute()
            .left(toward(stage.origin.x, card.origin.x))
            .top(toward(stage.origin.y, card.origin.y))
            .w(stage.size.width * scale)
            .h(px(height))
            .overflow_hidden()
            .rounded(card_radius * settled)
            .bg(background)
            .opacity(((1.0 - progress).abs() / CARD_FADE).min(1.0))
            .children(layers)
            .into_any_element()
    }

    fn reveal_current(
        &mut self,
        page: usize,
        section: usize,
        window: WindowId,
        card: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.compact.overview_rows.get(&(page, window)) else {
            return;
        };
        if row.bounds_for_item(card).is_none() {
            cx.notify();
            return;
        }
        self.compact.overview_reveal = false;
        row.scroll_to_item(card);
        if let Some(scroll) = self.compact.host_scrolls.get(&page) {
            scroll.scroll_to_item(section);
        }
    }

    fn overview_bar(
        &self,
        hosts: &[HostPage],
        index: usize,
        placement: &PillPlacement,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let host = hosts.get(index);
        let connected = host.is_some_and(|host| host.connected(cx));
        let meta = host.map_or_else(String::new, |host| host.detail(cx));
        let dots = (hosts.len() > 1).then(|| {
            page_dots(
                &[(0..hosts.len())
                    .map(|page| PageDot {
                        active: page == index,
                        attention: false,
                    })
                    .collect()],
                cx,
            )
        });
        let count = hosts.len();
        let pill = compact_floating_pill(
            "overview-host",
            PillLabel {
                icon: IconName::Server,
                title: host
                    .map_or_else(String::new, |host| host.title.clone())
                    .into(),
                meta: meta.into(),
                warning: false,
            },
            div().children(dots),
            placement,
            cx,
        )
        .when(count > 1, |pill| {
            pill.child(claim_back_swipe("overview-host-swipe"))
        })
        .on_click(cx.listener(|this, _, _, cx| {
            this.compact.hosts_sheet = true;
            cx.notify();
        }))
        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
            this.swipe_host(event, window, cx);
        }));
        let new = compact_bar_button("overview-new", IconName::Plus)
            .disabled(!connected)
            .on_click(cx.listener(|this, _, _, cx| {
                this.compact.new_sheet = true;
                cx.notify();
            }));
        let settings = compact_bar_button("overview-settings", IconName::Settings).on_click(
            cx.listener(|this, _, _, cx| {
                this.settings = Some(zz_ui::settings::SettingsSection::Appearance);
                this.focused_pane = None;
                cx.notify();
            }),
        );
        compact_floating_bar(pill, placement, cx)
            .child(compact_floating_group([new, settings], placement, cx))
            .into_any_element()
    }

    fn swipe_host(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta_x = match event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.x),
            ScrollDelta::Lines(delta) => delta.x * 20.0,
        };
        let response = self.compact.host_pager.scroll(
            delta_x,
            event.touch_phase,
            self.compact.width,
            Instant::now(),
        );
        if !response.consumed {
            return;
        }
        cx.stop_propagation();
        if self.compact.host_pager.swallows_momentum() {
            window.end_touch_momentum();
        }
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    fn overview_section(
        &mut self,
        page: usize,
        id: WindowId,
        index: u32,
        title: &str,
        folder: Option<&str>,
        cards: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scroll = self
            .compact
            .overview_rows
            .entry((page, id))
            .or_default()
            .clone();
        let row = ((page as u64) << 40) | id.0;
        div()
            .flex()
            .flex_col()
            .child(overview_section_heading(index, title, folder, cx))
            .child(
                div()
                    .relative()
                    .child(
                        div()
                            .id(("overview-row", id.0))
                            .flex()
                            .gap(px(OVERVIEW_CARD_GAP))
                            .px(px(16.0))
                            .py(px(2.0))
                            .overflow_x_scroll()
                            .restrict_scroll_to_axis()
                            .track_scroll(&scroll)
                            .children(cards),
                    )
                    .child(yield_back_swipe(("overview-row-yield", row), &scroll)),
            )
            .into_any_element()
    }

    fn overview_card(
        &self,
        page: &Page,
        current: bool,
        connection: &Entity<Connection>,
        target: CardTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.foreground.muted();
        let core = &connection.read(cx).core;
        let viewport = core.viewport(page.pane);
        let browser = matches!(target, CardTarget::Here)
            .then(|| self.browser_page(page, cx))
            .flatten();
        let tile = |icon: IconName| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .child(Icon::new(icon).size(px(20.0)))
                .into_any_element()
        };
        let preview = match &page.kind {
            PaneKindSnapshot::Terminal => Some(viewport.map_or_else(
                || tile(kind_icon(&page.kind)),
                |viewport| {
                    div()
                        .px(px(4.0))
                        .font_family(theme.mono_font_family.clone())
                        .text_size(px(PREVIEW_TEXT))
                        .line_height(px(PREVIEW_LINE))
                        .whitespace_nowrap()
                        .child(terminal_preview(
                            viewport,
                            phone_colors(viewport, &host_palette(core), cx),
                            &font(theme.mono_font_family.clone()),
                            0..viewport.rows.min(PREVIEW_ROWS),
                            PREVIEW_COLUMNS,
                        ))
                        .into_any_element()
                },
            )),
            PaneKindSnapshot::Browser(descriptor) => Some(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .text_size(px(9.0))
                    .text_color(muted)
                    .child(Icon::new(IconName::Globe).size(px(20.0)))
                    .children(
                        browser
                            .as_ref()
                            .map(|(_, host)| host.clone())
                            .or_else(|| descriptor.tabs.get(descriptor.active_tab).cloned())
                            .map(|url| {
                                div()
                                    .max_w_full()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(url)
                            }),
                    )
                    .into_any_element(),
            ),
            kind => Some(tile(kind_icon(kind))),
        };
        let pane_id = page.pane;
        let page = page.clone();
        let cards = Rc::clone(&self.compact.cards);
        let here = matches!(target, CardTarget::Here);
        overview_card(
            ("overview-card", pane_id.0),
            ("overview-card-press", pane_id.0),
            kind_icon(&page.kind),
            browser.map_or_else(|| page.title.clone(), |(title, _)| title),
            preview,
            current,
            page.attention,
            cx,
        )
        .when(here, |card| {
            card.child(
                canvas(
                    move |bounds, _, _| {
                        cards.borrow_mut().insert(pane_id, bounds);
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .inset_0(),
            )
        })
        .on_click(cx.listener(move |this, _, window, cx| {
            match &target {
                CardTarget::Here => {
                    this.compact_select(&page, cx);
                    this.land_overview(Some(page.pane), cx);
                    return;
                }
                CardTarget::Session(session) => this.open_pane(*session, page.pane, cx),
                #[cfg(any(target_os = "ios", target_os = "android"))]
                CardTarget::Host(endpoint, session) => {
                    this.switch_host(endpoint, Some(*session), Some(page.pane), window, cx);
                }
                #[cfg(not(any(target_os = "ios", target_os = "android")))]
                CardTarget::Host(..) => {}
            }
            let _ = window;
            this.close_overview(cx);
        }))
        .into_any_element()
    }

    fn open_pane(&mut self, session: SessionId, pane: PaneId, cx: &mut Context<Self>) {
        self.connection
            .update(cx, |connection, cx| connection.attach(session, cx));
        let target = pane.to_string();
        self.command("select-window", vec!["-t".into(), target.clone()], cx);
        self.command("select-pane", vec!["-Z".into(), "-t".into(), target], cx);
    }

    pub(in super::super) fn host_pages(&self, cx: &App) -> Vec<HostPage> {
        #[cfg(any(target_os = "ios", target_os = "android"))]
        {
            use super::super::hosts::host_title;
            let connections = self.host_connections();
            let saved = self.hosts.saved.endpoints();
            let mut pages: Vec<HostPage> = connections
                .iter()
                .filter_map(|connection| {
                    let endpoint = connection.read(cx).endpoint()?;
                    (!saved.iter().any(|saved| saved == endpoint)).then(|| HostPage {
                        endpoint: Some(endpoint.to_owned()),
                        title: host_title(endpoint),
                        connection: Some(connection.clone()),
                    })
                })
                .collect();
            pages.extend(saved.iter().map(|endpoint| {
                HostPage {
                    endpoint: Some(endpoint.clone()),
                    title: host_title(endpoint),
                    connection: connections
                        .iter()
                        .find(|connection| {
                            connection.read(cx).endpoint() == Some(endpoint.as_str())
                        })
                        .cloned(),
                }
            }));
            pages
        }
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        {
            let _ = cx;
            vec![HostPage {
                endpoint: None,
                title: "This computer".into(),
                connection: Some(self.connection.clone()),
            }]
        }
    }

    pub(in super::super) fn create(
        &mut self,
        what: NewThing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(any(target_os = "ios", target_os = "android"))]
        if let Some(endpoint) = self
            .host_pages(cx)
            .get(self.compact.host_pager.index())
            .and_then(|host| host.endpoint.clone())
            && !self.is_attached_host(&endpoint, cx)
        {
            self.switch_host(&endpoint, None, None, window, cx);
        }
        let _ = window;
        let here = || vec!["-c".to_owned(), "#{pane_current_path}".to_owned()];
        match what {
            NewThing::Terminal => self.command("new-window", here(), cx),
            NewThing::Agent => self.command(
                "new-window",
                [vec!["--kind".to_owned(), "agent".to_owned()], here()].concat(),
                cx,
            ),
            NewThing::Browser => {
                self.command("new-window", vec!["--kind".into(), "browser".into()], cx);
            }
            NewThing::Session => self.command("new-session", Vec::new(), cx),
        }
        self.compact.new_sheet = false;
        self.close_overview(cx);
    }

    pub(super) fn overview_hosts_sheet(
        &self,
        bottom_inset: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hosts = self.host_pages(cx);
        let shown = self.compact.host_pager.index();
        let rows: Vec<AnyElement> = hosts
            .iter()
            .enumerate()
            .map(|(index, host)| {
                let state = host.state(cx);
                let row = overview_host_row(
                    ("overview-host-option", index),
                    state,
                    host.title.clone(),
                    host.detail(cx),
                    index == shown,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.compact.host_pager.go_to(index, Instant::now());
                    close_sheet(HOSTS_SHEET, cx);
                    cx.notify();
                }));
                #[cfg(any(target_os = "ios", target_os = "android"))]
                let row = match (state, host.endpoint.clone()) {
                    (HostState::Offline, Some(endpoint)) => row.child(
                        Button::new(("overview-host-connect", index))
                            .label("Connect")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.connect_host(endpoint.clone(), window, cx);
                            })),
                    ),
                    (HostState::Connecting, Some(endpoint)) => row.child(
                        Button::new(("overview-host-cancel", index))
                            .label("Cancel")
                            .ghost()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.disconnect_host(&endpoint, window, cx);
                            })),
                    ),
                    _ => row,
                };
                row.into_any_element()
            })
            .collect();
        #[cfg(any(target_os = "ios", target_os = "android"))]
        let add = Some(
            sheet_row(
                "overview-add-host",
                roles::PHONE_ROW,
                Some(Icon::new(IconName::Plus)),
                "Add host",
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.compact.hosts_sheet = false;
                this.compact.add_host = true;
                cx.notify();
            })),
        );
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let add: Option<zz_gpui::Stateful<zz_gpui::Div>> = None;
        let close =
            sheet_close("overview-hosts-close").on_click(|_, _, cx| close_sheet(HOSTS_SHEET, cx));
        let dismiss = cx.weak_entity();
        bottom_sheet(
            HOSTS_SHEET,
            "Hosts",
            [close.into_any_element()],
            div()
                .flex()
                .flex_col()
                .list_inset(cx)
                .pb(px(8.0))
                .children(rows)
                .when(add.is_some(), |list| {
                    list.child(div().mx(px(12.0)).my(px(4.0)).rule_on(
                        roles::LIST_RULE,
                        Edge::Top,
                        cx,
                    ))
                })
                .children(add),
            bottom_inset,
            move |_, cx| {
                let _ = dismiss.update(cx, |this, cx| {
                    this.compact.hosts_sheet = false;
                    cx.notify();
                });
            },
        )
        .surface_role(roles::HOSTS_DRAWER)
        .header_role(roles::PHONE_BAR)
        .into_any_element()
    }

    pub(super) fn overview_new_sheet(
        &self,
        bottom_inset: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let host = self
            .host_pages(cx)
            .get(self.compact.host_pager.index())
            .cloned();
        let host_name = host
            .as_ref()
            .map_or_else(String::new, |host| host.title.clone());
        let session = host
            .as_ref()
            .and_then(|host| host.connection.as_ref())
            .and_then(|connection| {
                let connection = connection.read(cx);
                let core = &connection.core;
                let id = core
                    .attached_session()
                    .or_else(|| connection.remembered_session())?;
                core.snapshot()
                    .sessions
                    .iter()
                    .find(|session| session.id == id)
                    .map(|session| session_label(session).text)
            });
        let muted = cx.theme().foreground.muted();
        let place = match &session {
            Some(session) => format!("Opens as a new window in {session} on {host_name}."),
            None => format!("Opens as a new window on {host_name}."),
        };
        let choice = |id: &'static str,
                      icon: IconName,
                      title: &'static str,
                      detail: String,
                      what: NewThing,
                      cx: &mut Context<Self>| {
            overview_choice_row(id, Some(Icon::new(icon)), title, detail, cx)
                .on_click(cx.listener(move |this, _, window, cx| this.create(what, window, cx)))
                .into_any_element()
        };
        let rows = vec![
            choice(
                "overview-new-terminal",
                IconName::SquareTerminal,
                "Terminal",
                "A shell in the current folder".into(),
                NewThing::Terminal,
                cx,
            ),
            choice(
                "overview-new-agent",
                IconName::Bot,
                "Agent",
                "Claude Code or Codex in the current folder".into(),
                NewThing::Agent,
                cx,
            ),
            choice(
                "overview-new-browser",
                IconName::Globe,
                "Browser",
                format!("A web page on {host_name}, localhost included"),
                NewThing::Browser,
                cx,
            ),
        ];
        let new_session = overview_choice_row(
            "overview-new-session",
            Some(Icon::new(IconName::Layers)),
            "New session",
            format!("Its own set of windows on {host_name}"),
            cx,
        )
        .on_click(cx.listener(|this, _, window, cx| this.create(NewThing::Session, window, cx)));
        let close =
            sheet_close("overview-new-close").on_click(|_, _, cx| close_sheet(NEW_SHEET, cx));
        let dismiss = cx.weak_entity();
        bottom_sheet(
            NEW_SHEET,
            "New",
            [close.into_any_element()],
            div()
                .flex()
                .flex_col()
                .list_inset(cx)
                .pb(px(8.0))
                .child(
                    div()
                        .px(px(12.0))
                        .pb(px(10.0))
                        .text_size(px(13.0))
                        .line_height(px(18.0))
                        .text_color(muted)
                        .child(place),
                )
                .children(rows)
                .child(
                    div()
                        .mx(px(12.0))
                        .my(px(6.0))
                        .rule_on(roles::LIST_RULE, Edge::Top, cx),
                )
                .child(new_session),
            bottom_inset,
            move |_, cx| {
                let _ = dismiss.update(cx, |this, cx| {
                    this.compact.new_sheet = false;
                    cx.notify();
                });
            },
        )
        .surface_role(roles::NEW_DRAWER)
        .header_role(roles::PHONE_BAR)
        .into_any_element()
    }

    #[cfg(any(target_os = "ios", target_os = "android"))]
    pub(super) fn overview_add_host_sheet(
        &self,
        bottom_inset: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().foreground.muted();
        let close = sheet_close("overview-add-host-close")
            .on_click(|_, _, cx| close_sheet(ADD_HOST_SHEET, cx));
        let dismiss = cx.weak_entity();
        bottom_sheet(
            ADD_HOST_SHEET,
            "Add host",
            [close.into_any_element()],
            div()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .px(px(16.0))
                .pb(px(16.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(div().flex_1().min_w_0().child(Input::new(&self.hosts.destination)))
                        .child(
                            Button::new("overview-add-host-connect")
                                .primary()
                                .label("Connect")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if this.add_host(window, cx) {
                                        this.compact.add_host = false;
                                        let count = this.host_pages(cx).len();
                                        let shown = this.compact.host_pager.index();
                                        this.compact.host_pager.sync(shown, count);
                                        this.compact
                                            .host_pager
                                            .go_to(count.saturating_sub(1), Instant::now());
                                        cx.notify();
                                    }
                                })),
                        ),
                )
                .children(self.hosts.error.clone().map(|error| {
                    div()
                        .text_size(px(13.0))
                        .text_color(cx.theme().warning)
                        .child(error)
                }))
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(muted)
                        .child("user@host, or user@host:port. Hosts you are on stay connected."),
                )
                .child(div().rule_on(roles::LIST_RULE, Edge::Top, cx))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(13.0))
                                .text_color(muted)
                                .child("Add this key to ~/.ssh/authorized_keys on the host to sign in without a password."),
                        )
                        .child(
                            Button::new("overview-add-host-copy-key")
                                .label("Copy key")
                                .on_click(|_, window, cx| super::super::hosts::copy_ssh_key(window, cx)),
                        ),
                ),
            bottom_inset,
            move |_, cx| {
                let _ = dismiss.update(cx, |this, cx| {
                    this.compact.add_host = false;
                    cx.notify();
                });
            },
        )
        .surface_role(roles::ADD_HOST_DRAWER)
        .header_role(roles::PHONE_BAR)
        .into_any_element()
    }
}

#[derive(Clone)]
enum CardTarget {
    Here,
    Session(SessionId),
    #[cfg_attr(not(any(target_os = "ios", target_os = "android")), allow(dead_code))]
    Host(String, SessionId),
}

struct WindowTitle {
    id: WindowId,
    title: String,
    folder: Option<String>,
}

#[derive(Clone, Copy)]
pub(in super::super) enum NewThing {
    Terminal,
    Agent,
    Browser,
    Session,
}

#[derive(Clone)]
pub(in super::super) struct HostPage {
    pub endpoint: Option<String>,
    pub title: String,
    pub connection: Option<Entity<Connection>>,
}

impl HostPage {
    fn connected(&self, cx: &App) -> bool {
        self.connection
            .as_ref()
            .is_some_and(|connection| connection.read(cx).connected)
    }

    fn state(&self, cx: &App) -> HostState {
        match &self.connection {
            Some(connection) if connection.read(cx).connected => HostState::Connected,
            Some(connection) if connection.read(cx).busy_or_connecting() => HostState::Connecting,
            _ => HostState::Offline,
        }
    }

    fn detail(&self, cx: &App) -> String {
        match &self.connection {
            Some(connection) if connection.read(cx).connected => overview_count(
                connection.read(cx).core.snapshot().sessions.len(),
                "session",
            ),
            Some(connection) if connection.read(cx).busy_or_connecting() => "Connecting…".into(),
            _ => "Not connected".into(),
        }
    }
}
