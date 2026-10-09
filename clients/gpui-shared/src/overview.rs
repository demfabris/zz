use std::{ops::Range, rc::Rc};

use zpui::{
    AnyElement, Bounds, Context, Font, Hsla, ParentElement as _, Pixels, StyledText, TextRun,
    Window, canvas, div, font, point, prelude::*, px, size,
};
use zz_client::StatusBarModel;
use zz_protocol::{PaneId, PaneKindSnapshot, WindowId};
use zz_terminal::{Glyph, TerminalViewport};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, StyledExt as _,
    button::{Button, ButtonVariants as _},
    compact::{
        COMPACT_PANE_HEADER_HEIGHT, Instant, bottom_sheet, sheet_action, sheet_close, sheet_option,
        yield_back_swipe,
    },
    rems_from_px,
    terminal::terminal_background,
    touch::press_highlight,
};

use super::{AppShell, Page, kind_icon, pages};

const TOP_BAR: f32 = 44.0;
const BUTTON: f32 = 40.0;
const CARD_WIDTH: f32 = 108.0;
const CARD_HEIGHT: f32 = 192.0;
const ADD_WIDTH: f32 = 76.0;
const CARD_GAP: f32 = 10.0;
const CARD_RADIUS: f32 = 8.0;
const CARD_STRIP: f32 = 20.0;
const PREVIEW_TEXT: f32 = 4.0;
const PREVIEW_LINE: f32 = 4.8;
const PREVIEW_ROWS: u16 = 40;
const PREVIEW_COLUMNS: usize = 56;
const PRESS_WASH: f32 = 0.08;
const OVERVIEW_FADE: f32 = 1.5;
const LIVE_FADE: f32 = 3.0;
const CARD_FADE: f32 = 0.15;
const TEXT_STEP: f32 = 4.0;

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn quantized(size: Pixels) -> Pixels {
    px((f32::from(size) * TEXT_STEP).round() / TEXT_STEP)
}

fn terminal_preview(
    viewport: &TerminalViewport,
    base: &Font,
    rows: Range<u16>,
    columns: usize,
) -> StyledText {
    let fallback = terminal_background(viewport.foreground, 1.0);
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
            self.compact.overview = true;
            self.compact.overview_reveal = true;
            self.compact.landing = None;
            self.compact.lift.animate(true, Instant::now());
        }
        cx.notify();
    }

    pub(in super::super) fn close_overview(&mut self, cx: &mut Context<Self>) {
        self.compact.overview = false;
        self.compact.sessions = false;
        self.compact.landing = None;
        self.compact.lift.snap(false);
        self.focused_pane = None;
        cx.notify();
    }

    pub(in super::super) fn land_overview(&mut self, pane: Option<PaneId>, cx: &mut Context<Self>) {
        let now = Instant::now();
        self.compact.overview = false;
        self.compact.sessions = false;
        self.compact.landing = pane.map(|pane| (pane, now));
        self.compact.lift.animate(false, now);
        self.focused_pane = None;
        cx.notify();
    }

    pub(super) fn compact_overview(
        &mut self,
        model: &StatusBarModel,
        top_inset: Pixels,
        bottom_inset: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (pages, current) = pages(model, &self.unseen_agents);
        let shown = pages.get(current).map(|page| page.window);
        let current = pages.get(current).map(|page| page.pane);
        self.compact
            .overview_rows
            .retain(|window, _| model.windows.iter().any(|shown| shown.id == *window));
        self.compact
            .cards
            .borrow_mut()
            .retain(|pane, _| pages.iter().any(|page| page.pane == *pane));
        let sections: Vec<AnyElement> = model
            .windows
            .iter()
            .map(|window| {
                let cards: Vec<AnyElement> = pages
                    .iter()
                    .filter(|page| page.window == window.id)
                    .map(|page| self.overview_card(page, current == Some(page.pane), cx))
                    .collect();
                self.overview_section(
                    window.id,
                    window.index,
                    &window.name,
                    window.panes.len(),
                    cards,
                    cx,
                )
            })
            .collect();
        if self.compact.overview_reveal {
            self.reveal_current(model, &pages, shown, current, cx);
        }
        let theme = cx.theme();
        let muted = theme.foreground.muted();
        let wash = theme.foreground.opacity(PRESS_WASH);
        let radius = theme.radius;
        let new_window = div()
            .id("overview-new-window")
            .relative()
            .flex()
            .items_center()
            .gap(px(10.0))
            .h(px(44.0))
            .mx(px(8.0))
            .mt(px(18.0))
            .px(px(12.0))
            .child(press_highlight("overview-new-window-press", wash, radius))
            .child(Icon::new(IconName::Plus).size(px(16.0)).text_color(muted))
            .child(div().text_size(px(15.0)).child("New window"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.command(
                    "new-window",
                    vec!["-c".into(), "#{pane_current_path}".into()],
                    cx,
                );
                this.close_overview(cx);
            }));
        let page = div()
            .size_full()
            .flex()
            .flex_col()
            .pt(top_inset)
            .bg(cx.theme().background)
            .child(self.overview_top_bar(model, cx))
            .child(
                div()
                    .id("overview-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.compact.overview_scroll)
                    .pb(bottom_inset + px(16.0))
                    .children(sections)
                    .child(new_window),
            );
        page.into_any_element()
    }

    pub(super) fn overview_lift(
        &mut self,
        model: &StatusBarModel,
        stage: Bounds<Pixels>,
        top_inset: Pixels,
        bottom_inset: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> zpui::Div {
        let progress = self.compact.lift.progress();
        let (pages, current) = pages(model, &self.unseen_agents);
        let target = self
            .compact
            .landing
            .map(|(pane, _)| pane)
            .or_else(|| pages.get(current).map(|page| page.pane));
        let overview = self.compact_overview(model, top_inset, bottom_inset, cx);
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
                    point(px(16.0), top_inset + px(TOP_BAR * 2.0)),
                    size(px(CARD_WIDTH), px(CARD_HEIGHT)),
                )
            });
        let settled = progress.min(1.0);
        let rest = f32::from(card.size.width) / f32::from(stage.size.width);
        let scale = 1.0 + (rest - 1.0) * progress;
        let tall = f32::from(stage.size.height);
        let height = (tall + (f32::from(card.size.height) / rest - tall) * settled) * scale;
        let toward = |from: Pixels, to: Pixels| from + (to - from) * progress;
        let theme = cx.theme();
        let muted = theme.foreground.muted();
        let mut background = theme.background;
        let mut layers: Vec<AnyElement> = Vec::new();
        let text = self.compact.text.clone();
        let terminal = {
            let core = &self.connection.read(cx).core;
            match (&page.kind, core.viewport(page.pane), text) {
                (PaneKindSnapshot::Terminal, Some(viewport), Some(text)) => {
                    let first = viewport.rows.saturating_sub(text.rows);
                    Some((
                        terminal_background(viewport.background, 1.0),
                        terminal_preview(
                            viewport,
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
        if let Some((fill, preview, text)) = terminal {
            background = fill;
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
                let element = self.compact_page(page, false, 0.0, top_inset, window, cx);
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
            .rounded(px(CARD_RADIUS * settled))
            .bg(background)
            .opacity(((1.0 - progress).abs() / CARD_FADE).min(1.0))
            .children(layers)
            .into_any_element()
    }

    fn reveal_current(
        &mut self,
        model: &StatusBarModel,
        pages: &[Page],
        shown: Option<WindowId>,
        current: Option<zz_protocol::PaneId>,
        cx: &mut Context<Self>,
    ) {
        let target = shown.and_then(|window| {
            let section = model.windows.iter().position(|row| row.id == window)?;
            let card = pages
                .iter()
                .filter(|page| page.window == window)
                .position(|page| Some(page.pane) == current)?;
            Some((window, section, card))
        });
        let Some((window, section, card)) = target else {
            self.compact.overview_reveal = false;
            return;
        };
        let Some(row) = self.compact.overview_rows.get(&window) else {
            return;
        };
        if row.bounds_for_item(card).is_none() {
            cx.notify();
            return;
        }
        self.compact.overview_reveal = false;
        self.compact.overview_scroll.scroll_to_item(section);
        row.scroll_to_item(card + 1);
    }

    fn overview_top_bar(&self, model: &StatusBarModel, cx: &mut Context<Self>) -> AnyElement {
        #[cfg(target_os = "ios")]
        let host = self
            .connection
            .read(cx)
            .endpoint()
            .map(super::super::hosts::host_title);
        #[cfg(not(target_os = "ios"))]
        let host: Option<String> = None;
        let theme = cx.theme();
        let muted = theme.foreground.muted();
        let switcher = div()
            .id("overview-session")
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.0))
            .h(px(BUTTON))
            .px(px(8.0))
            .child(press_highlight(
                "overview-session-press",
                theme.foreground.opacity(PRESS_WASH),
                theme.radius,
            ))
            .child(Icon::new(IconName::Layers).size(px(16.0)).text_color(muted))
            .child(
                div()
                    .text_size(px(17.0))
                    .font_semibold()
                    .whitespace_nowrap()
                    .child(model.session_name.clone().unwrap_or_default()),
            )
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(px(14.0))
                    .text_color(muted),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.compact.sessions = true;
                cx.notify();
            }));
        let settings = Button::new("overview-settings")
            .ghost()
            .compact()
            .tab_stop(false)
            .size(px(BUTTON))
            .child(Icon::new(IconName::Settings).size(px(18.0)))
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings = Some(zz_ui::settings::SettingsSection::Appearance);
                this.focused_pane = None;
                cx.notify();
            }));
        let current = self.active_window(cx).map(|window| window.active_pane);
        let close = sheet_close("overview-close")
            .on_click(cx.listener(move |this, _, _, cx| this.land_overview(current, cx)));
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.0))
            .h(px(TOP_BAR))
            .px(px(8.0))
            .child(switcher)
            .children(host.map(|host| {
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(13.0))
                    .text_color(muted)
                    .child(host)
            }))
            .child(div().flex_1())
            .child(settings)
            .child(close)
            .into_any_element()
    }

    fn overview_section(
        &mut self,
        id: WindowId,
        index: u32,
        name: &str,
        panes: usize,
        cards: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let scroll = self.compact.overview_rows.entry(id).or_default().clone();
        let theme = cx.theme();
        let muted = theme.foreground.muted();
        let add = div()
            .id(("overview-add", id.0))
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .w(px(ADD_WIDTH))
            .h(px(CARD_HEIGHT))
            .rounded(px(CARD_RADIUS))
            .bg(theme.background.raised(1))
            .control_surface(cx)
            .text_size(px(12.0))
            .text_color(muted)
            .child(Icon::new(IconName::Plus).size(px(16.0)))
            .child("New pane")
            .child(press_highlight(
                ("overview-add-press", id.0),
                theme.foreground.opacity(PRESS_WASH),
                px(CARD_RADIUS),
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.command("select-window", vec!["-t".into(), id.to_string()], cx);
                this.command(
                    "split-window",
                    vec![
                        "-h".into(),
                        "-t".into(),
                        id.to_string(),
                        "-c".into(),
                        "#{pane_current_path}".into(),
                    ],
                    cx,
                );
                this.close_overview(cx);
            }));
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.0))
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_size(px(13.0))
                    .child(div().text_color(muted).child(index.to_string()))
                    .child(div().font_medium().child(name.to_owned()))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(muted)
                            .child(count(panes, "pane")),
                    ),
            )
            .child(
                div()
                    .relative()
                    .child(
                        div()
                            .id(("overview-row", id.0))
                            .flex()
                            .gap(px(CARD_GAP))
                            .px(px(16.0))
                            .py(px(2.0))
                            .overflow_x_scroll()
                            .track_scroll(&scroll)
                            .children(cards)
                            .child(add),
                    )
                    .child(yield_back_swipe(("overview-row-yield", id.0), &scroll)),
            )
            .into_any_element()
    }

    fn overview_card(&self, page: &Page, current: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.foreground.muted();
        let core = &self.connection.read(cx).core;
        let viewport = core.viewport(page.pane);
        let mut background = theme.background;
        let preview = match &page.kind {
            PaneKindSnapshot::Terminal => viewport.map(|viewport| {
                background = terminal_background(viewport.background, 1.0);
                div()
                    .px(px(4.0))
                    .font_family(theme.mono_font_family.clone())
                    .text_size(px(PREVIEW_TEXT))
                    .line_height(px(PREVIEW_LINE))
                    .whitespace_nowrap()
                    .child(terminal_preview(
                        viewport,
                        &font(theme.mono_font_family.clone()),
                        0..viewport.rows.min(PREVIEW_ROWS),
                        PREVIEW_COLUMNS,
                    ))
                    .into_any_element()
            }),
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
                    .children(descriptor.tabs.get(descriptor.active_tab).map(|url| {
                        div()
                            .max_w_full()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(url.clone())
                    }))
                    .into_any_element(),
            ),
            kind => Some(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(muted)
                    .child(Icon::new(kind_icon(kind)).size(px(20.0)))
                    .into_any_element(),
            ),
        };
        let pane_id = page.pane;
        let target = page.clone();
        let cards = Rc::clone(&self.compact.cards);
        div()
            .id(("overview-card", pane_id.0))
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(CARD_WIDTH))
            .h(px(CARD_HEIGHT))
            .rounded(px(CARD_RADIUS))
            .overflow_hidden()
            .bg(background)
            .control_surface(cx)
            .when(current, |card| card.border_2().border_color(theme.accent))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(4.0))
                    .h(px(CARD_STRIP))
                    .px(px(6.0))
                    .text_size(px(11.0))
                    .text_color(muted)
                    .child(Icon::new(kind_icon(&page.kind)).size(px(11.0)))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(page.title.clone()),
                    ),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().children(preview))
            .when(page.attention && !current, |card| {
                card.child(
                    div()
                        .absolute()
                        .top(px(7.0))
                        .right(px(6.0))
                        .size(px(6.0))
                        .rounded_full()
                        .bg(theme.warning),
                )
            })
            .child(press_highlight(
                ("overview-card-press", pane_id.0),
                theme.foreground.opacity(PRESS_WASH),
                px(CARD_RADIUS),
            ))
            .child(
                canvas(
                    move |bounds, _, _| {
                        cards.borrow_mut().insert(pane_id, bounds);
                    },
                    |_, (), _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.compact_select(&target, cx);
                this.land_overview(Some(target.pane), cx);
            }))
            .into_any_element()
    }

    pub(super) fn overview_sessions_sheet(
        &self,
        bottom_inset: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let core = &self.connection.read(cx).core;
        let attached = core.attached_session();
        let muted = cx.theme().foreground.muted();
        let rows: Vec<AnyElement> = core
            .snapshot()
            .sessions
            .iter()
            .map(|session| {
                let id = session.id;
                sheet_option(
                    ("overview-session-option", id.0),
                    session.name.clone(),
                    attached == Some(id),
                    cx,
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(13.0))
                        .text_color(muted)
                        .child(count(session.windows.len(), "window")),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.compact.sessions = false;
                    if this.connection.read(cx).core.attached_session() != Some(id) {
                        this.connection
                            .update(cx, |connection, cx| connection.attach(id, cx));
                    }
                    cx.notify();
                }))
                .into_any_element()
            })
            .collect();
        let new_session = sheet_action("overview-new-session", IconName::Plus, "New session", cx)
            .on_click(cx.listener(|this, _, _, cx| {
                this.compact.sessions = false;
                this.command("new-session", Vec::new(), cx);
                cx.notify();
            }));
        let close =
            sheet_close("overview-sessions-close").on_click(cx.listener(|this, _, _, cx| {
                this.compact.sessions = false;
                cx.notify();
            }));
        let dismiss = cx.weak_entity();
        bottom_sheet(
            "overview-sessions-sheet",
            "Sessions",
            [close.into_any_element()],
            div()
                .flex()
                .flex_col()
                .px(px(8.0))
                .pb(px(8.0))
                .children(rows)
                .child(
                    div()
                        .h(px(1.0))
                        .mx(px(12.0))
                        .my(px(4.0))
                        .bg(cx.theme().border()),
                )
                .child(new_session),
            bottom_inset,
            move |_, cx| {
                let _ = dismiss.update(cx, |this, cx| {
                    this.compact.sessions = false;
                    cx.notify();
                });
            },
        )
        .into_any_element()
    }
}
