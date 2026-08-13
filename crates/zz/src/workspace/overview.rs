use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, App, Bounds, Context, EventEmitter, FocusHandle, Focusable,
    IntoElement, KeyBinding, KeyDownEvent, MouseButton, Pixels, Render, Size, Window, div,
    ease_out_quint, point, prelude::*, px, relative, size,
};
use zz_protocol::{PaneKindSnapshot, WindowId, WindowSnapshot};
use zz_ui::{
    ActiveTheme as _, Colorize as _, Icon, IconName, Sizable as _, StyledExt as _,
    command::palette_shortcut_hint,
};

use crate::{
    mux::{client::MuxClient, nav::select_window_command},
    pane::layout::pane_rects,
};

use super::sidebar::WorkspaceRoute;

const OVERVIEW_KEY_CONTEXT: &str = "WindowOverview";
const OVERVIEW_OPEN: Duration = Duration::from_millis(260);
const OVERVIEW_CLOSE: Duration = Duration::from_millis(210);
const CARD_ASPECT_RATIO: f32 = 1.6;
const SIDE_PADDING: f32 = 32.0;
const TOP_PADDING: f32 = 76.0;
const BOTTOM_PADDING: f32 = 54.0;
const CARD_GAP: f32 = 18.0;

gpui::actions!(zz, [ToggleWindowOverview]);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys(key_bindings());
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) fn key_bindings() -> [KeyBinding; 1] {
    [KeyBinding::new(
        "cmd-shift-o",
        ToggleWindowOverview,
        Some(zz_ui::ROOT_KEY_CONTEXT),
    )]
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub(crate) fn key_bindings() -> [KeyBinding; 1] {
    [KeyBinding::new(
        "ctrl-shift-o",
        ToggleWindowOverview,
        Some(zz_ui::ROOT_KEY_CONTEXT),
    )]
}

pub(crate) struct DismissWindowOverview;

#[derive(Clone, Copy)]
struct Closing {
    focus: WindowId,
}

pub(crate) struct WindowOverview {
    mux: gpui::Entity<MuxClient>,
    focus_handle: FocusHandle,
    selected: Option<WindowId>,
    closing: Option<Closing>,
    animation_revision: u64,
}

impl WindowOverview {
    pub(crate) fn new(mux: gpui::Entity<MuxClient>, cx: &mut Context<Self>) -> Self {
        cx.observe(&mux, |_, _, cx| cx.notify()).detach();
        let selected = focused_window(mux.read(cx));
        Self {
            mux,
            focus_handle: cx.focus_handle(),
            selected,
            closing: None,
            animation_revision: 0,
        }
    }

    pub(crate) fn focus(&self) -> &FocusHandle {
        &self.focus_handle
    }

    pub(crate) fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.begin_close(None, window, cx);
    }

    fn activate(&mut self, target: WindowId, window: &mut Window, cx: &mut Context<Self>) {
        self.begin_close(Some(target), window, cx);
    }

    fn begin_close(
        &mut self,
        target: Option<WindowId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing.is_some() {
            return;
        }
        let focus = target
            .or_else(|| focused_window(self.mux.read(cx)))
            .or(self.selected);
        let Some(focus) = focus else {
            cx.emit(DismissWindowOverview);
            return;
        };
        self.closing = Some(Closing { focus });
        self.animation_revision = self.animation_revision.wrapping_add(1);
        if let Some(target) = target {
            self.mux.read(cx).execute(select_window_command(target));
        }
        cx.notify();
        let duration = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            OVERVIEW_CLOSE
        };
        cx.spawn_in(window, async move |view, cx| {
            cx.background_executor().timer(duration).await;
            let _ = view.update_in(cx, |_, _, cx| cx.emit(DismissWindowOverview));
        })
        .detach();
    }

    fn select(&mut self, selected: WindowId, cx: &mut Context<Self>) {
        if self.closing.is_none() && self.selected != Some(selected) {
            self.selected = Some(selected);
            cx.notify();
        }
    }

    fn move_selection(&mut self, direction: OverviewMove, window: &Window, cx: &mut Context<Self>) {
        if self.closing.is_some() {
            return;
        }
        let mux = self.mux.read(cx);
        let snapshot = mux.snapshot();
        let Some(attached) = mux.attached_session() else {
            return;
        };
        let Some(session) = snapshot
            .sessions
            .iter()
            .find(|session| session.id == attached)
        else {
            return;
        };
        let count = session.windows.len();
        if count == 0 {
            return;
        }
        let grid = OverviewGrid::new(window.viewport_size(), count);
        let current = self
            .selected
            .and_then(|selected| {
                session
                    .windows
                    .iter()
                    .position(|candidate| candidate.id == selected)
            })
            .unwrap_or(0);
        let next = moved_index(current, count, grid.columns, direction);
        self.selected = session.windows.get(next).map(|window| window.id);
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" | "q" => self.dismiss(window, cx),
            "enter" | "space" => {
                if let Some(selected) = self.selected {
                    self.activate(selected, window, cx);
                }
            }
            "left" | "h" => self.move_selection(OverviewMove::Left, window, cx),
            "right" | "l" => self.move_selection(OverviewMove::Right, window, cx),
            "up" | "k" => self.move_selection(OverviewMove::Up, window, cx),
            "down" | "j" => self.move_selection(OverviewMove::Down, window, cx),
            "home" => self.move_selection(OverviewMove::First, window, cx),
            "end" => self.move_selection(OverviewMove::Last, window, cx),
            "tab" if event.keystroke.modifiers.shift => {
                self.move_selection(OverviewMove::Previous, window, cx);
            }
            "tab" => self.move_selection(OverviewMove::Next, window, cx),
            _ => {}
        }
        cx.stop_propagation();
    }

    fn render_window_card(
        &self,
        mux_window: &WindowSnapshot,
        target: Bounds<Pixels>,
        focus_bounds: Bounds<Pixels>,
        index: usize,
        current: bool,
        selected: bool,
        closing: Option<Closing>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = mux_window.id;
        let pane_count = mux_window.panes.len();
        let title = window_label(mux_window);
        let panes = pane_rects(&mux_window.layout)
            .into_iter()
            .filter_map(|(pane, rect)| {
                let snapshot = mux_window.panes.get(&pane)?;
                let active = pane == mux_window.active_pane;
                let fill = if active {
                    cx.theme().background.raised(3)
                } else {
                    cx.theme().background.raised(2)
                };
                Some(
                    div()
                        .absolute()
                        .left(relative(rect.left()))
                        .top(relative(rect.top()))
                        .w(relative(rect.width()))
                        .h(relative(rect.height()))
                        .p(px(2.0))
                        .child(
                            div()
                                .id(("window-overview-pane", pane.0))
                                .flex()
                                .size_full()
                                .min_w_0()
                                .items_center()
                                .gap_1()
                                .overflow_hidden()
                                .rounded(cx.theme().radius)
                                .border_1()
                                .border_color(if active {
                                    cx.theme().foreground.muted()
                                } else {
                                    cx.theme().border.subtle()
                                })
                                .bg(fill)
                                .px(px(6.0))
                                .text_size(zz_ui::rems_from_px(10.0))
                                .text_color(if active {
                                    cx.theme().foreground
                                } else {
                                    cx.theme().foreground.muted()
                                })
                                .child(Icon::new(pane_kind_icon(&snapshot.kind)).xsmall())
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .child(crate::mux::nav::pane_label(snapshot)),
                                ),
                        ),
                )
            })
            .collect::<Vec<_>>();
        let card = div()
            .id(("window-overview-card", id.0))
            .absolute()
            .flex()
            .flex_col()
            .overflow_hidden()
            .cursor_pointer()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(if selected {
                cx.theme().foreground
            } else {
                cx.theme().border
            })
            .bg(cx.theme().background.raised(1).opaque())
            .shadow_lg()
            .hover(|card| card.bg(cx.theme().background.raised(2).opaque()))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .h(px(38.0))
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .px(px(12.0))
                    .border_b_1()
                    .border_color(cx.theme().border.subtle())
                    .child(Icon::new(IconName::AppWindow).small())
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .font_semibold()
                            .child(title),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(cx.theme().foreground.muted())
                            .child(format!(
                                "{pane_count} {}",
                                if pane_count == 1 { "pane" } else { "panes" }
                            )),
                    )
                    .when(current, |header| {
                        header.child(
                            div()
                                .flex_none()
                                .rounded_full()
                                .bg(cx.theme().foreground.wash())
                                .px(px(6.0))
                                .py(px(2.0))
                                .text_size(zz_ui::rems_from_px(9.0))
                                .child("current"),
                        )
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .bg(cx.theme().background)
                    .children(panes),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_hover(cx.listener(move |view, hovered, _, cx| {
                if *hovered {
                    view.select(id, cx);
                }
            }))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.activate(id, window, cx);
                cx.stop_propagation();
            }));
        let opening_start = if current {
            focus_bounds
        } else {
            stacked_bounds(focus_bounds, index)
        };
        let revision = self.animation_revision;
        let animation = Animation::new(if closing.is_some() {
            OVERVIEW_CLOSE
        } else {
            OVERVIEW_OPEN
        })
        .with_easing(ease_out_quint());
        card.with_animation(
            format!("window-overview-card-{}-{revision}", id.0),
            animation,
            move |card, delta| {
                let (bounds, opacity) = if let Some(closing) = closing {
                    if id == closing.focus {
                        (lerp_bounds(target, focus_bounds, delta), 1.0)
                    } else {
                        (
                            lerp_bounds(target, stacked_bounds(focus_bounds, index), delta),
                            1.0 - delta,
                        )
                    }
                } else {
                    (
                        lerp_bounds(opening_start, target, delta),
                        if current { 1.0 } else { delta },
                    )
                };
                card.left(bounds.origin.x)
                    .top(bounds.origin.y)
                    .w(bounds.size.width)
                    .h(bounds.size.height)
                    .opacity(opacity)
            },
        )
        .into_any_element()
    }
}

impl EventEmitter<DismissWindowOverview> for WindowOverview {}

impl Focusable for WindowOverview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for WindowOverview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mux = self.mux.read(cx);
        let snapshot = mux.snapshot();
        let attached = mux.attached_session();
        let session = attached.and_then(|attached| {
            snapshot
                .sessions
                .iter()
                .find(|session| session.id == attached)
        });
        let active = session.map(|session| snapshot.focused_window_for(session));
        let windows = session.map_or(&[][..], |session| session.windows.as_slice());
        if self
            .selected
            .is_none_or(|selected| !windows.iter().any(|window| window.id == selected))
        {
            self.selected = active.or_else(|| windows.first().map(|window| window.id));
        }
        let grid = OverviewGrid::new(window.viewport_size(), windows.len());
        let closing = self.closing;
        let cards = windows
            .iter()
            .enumerate()
            .map(|(index, mux_window)| {
                self.render_window_card(
                    mux_window,
                    grid.cards[index],
                    grid.focus,
                    index,
                    active == Some(mux_window.id),
                    self.selected == Some(mux_window.id),
                    closing,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        let session_name = session.map_or("Windows", |session| session.name.as_str());
        let count = windows.len();
        let revision = self.animation_revision;
        let closing_now = closing.is_some();
        let chrome_animation = Animation::new(if closing_now {
            OVERVIEW_CLOSE
        } else {
            OVERVIEW_OPEN
        })
        .with_easing(ease_out_quint());
        let backdrop = div()
            .absolute()
            .inset_0()
            .bg(cx.theme().background.floating())
            .with_animation(
                format!("window-overview-backdrop-{revision}"),
                chrome_animation.clone(),
                move |backdrop, delta| {
                    backdrop.opacity(if closing_now { 1.0 - delta } else { delta })
                },
            );
        let header = div()
            .absolute()
            .top(px(22.0))
            .left(px(SIDE_PADDING))
            .right(px(SIDE_PADDING))
            .flex()
            .items_end()
            .justify_between()
            .child(div().text_lg().font_semibold().child("Windows"))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().foreground.muted())
                    .child(format!(
                        "{session_name} · {count} {}",
                        if count == 1 { "window" } else { "windows" }
                    )),
            )
            .with_animation(
                format!("window-overview-header-{revision}"),
                chrome_animation.clone(),
                move |header, delta| header.opacity(if closing_now { 1.0 - delta } else { delta }),
            );
        let footer = div()
            .absolute()
            .bottom(px(16.0))
            .left(px(SIDE_PADDING))
            .right(px(SIDE_PADDING))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(18.0))
            .text_size(zz_ui::rems_from_px(10.0))
            .text_color(cx.theme().foreground.muted())
            .child(palette_shortcut_hint(
                ["up", "down", "left", "right"],
                "Move",
            ))
            .child(palette_shortcut_hint(["enter"], "Open"))
            .child(palette_shortcut_hint(["escape"], "Close"))
            .with_animation(
                format!("window-overview-footer-{revision}"),
                chrome_animation,
                move |footer, delta| footer.opacity(if closing_now { 1.0 - delta } else { delta }),
            );
        div()
            .id("window-overview")
            .key_context(OVERVIEW_KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .absolute()
            .inset_0()
            .occlude()
            .overflow_hidden()
            .text_color(cx.theme().foreground)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, window, cx| {
                    view.dismiss(window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(backdrop)
            .child(header)
            .children(cards)
            .child(footer)
    }
}

fn focused_window(mux: &MuxClient) -> Option<WindowId> {
    let attached = mux.attached_session()?;
    let snapshot = mux.snapshot();
    snapshot
        .sessions
        .iter()
        .find(|session| session.id == attached)
        .map(|session| snapshot.focused_window_for(session))
}

fn window_label(window: &WindowSnapshot) -> String {
    let name = window.name.trim();
    let name = if name.is_empty() {
        window
            .panes
            .get(&window.active_pane)
            .map_or_else(|| window.id.to_string(), crate::mux::nav::pane_label)
    } else {
        name.to_owned()
    };
    format!("{}:{name}", window.index)
}

const fn pane_kind_icon(kind: &PaneKindSnapshot) -> IconName {
    match kind {
        PaneKindSnapshot::Picker => IconName::Plus,
        PaneKindSnapshot::Terminal => IconName::SquareTerminal,
        PaneKindSnapshot::Browser(_) => IconName::Globe,
        PaneKindSnapshot::Agent(_) => IconName::Bot,
        PaneKindSnapshot::Editor(_) => IconName::File,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverviewMove {
    Left,
    Right,
    Up,
    Down,
    First,
    Last,
    Previous,
    Next,
}

fn moved_index(current: usize, count: usize, columns: usize, direction: OverviewMove) -> usize {
    if count == 0 {
        return 0;
    }
    match direction {
        OverviewMove::Left | OverviewMove::Previous => current.saturating_sub(1),
        OverviewMove::Right | OverviewMove::Next => (current + 1).min(count - 1),
        OverviewMove::Up => current.saturating_sub(columns),
        OverviewMove::Down => (current + columns).min(count - 1),
        OverviewMove::First => 0,
        OverviewMove::Last => count - 1,
    }
}

#[derive(Clone, Debug)]
struct OverviewGrid {
    columns: usize,
    cards: Vec<Bounds<Pixels>>,
    focus: Bounds<Pixels>,
}

impl OverviewGrid {
    fn new(viewport: Size<Pixels>, count: usize) -> Self {
        let viewport_width = f32::from(viewport.width).max(1.0);
        let viewport_height = f32::from(viewport.height).max(1.0);
        let side_padding = SIDE_PADDING.min(viewport_width * 0.08);
        let top_padding = TOP_PADDING.min(viewport_height * 0.24);
        let bottom_padding = BOTTOM_PADDING.min(viewport_height * 0.18);
        let content_width = (viewport_width - side_padding * 2.0).max(1.0);
        let content_height = (viewport_height - top_padding - bottom_padding).max(1.0);
        let gap = CARD_GAP.min((content_width / count.max(1) as f32).max(4.0));
        let mut columns = 1;
        let mut rows = count.max(1);
        let mut card_width = content_width;
        let mut card_height = content_height.min(card_width / CARD_ASPECT_RATIO);
        let mut best_area = 0.0;
        let mut best_empty = usize::MAX;
        for candidate_columns in 1..=count.max(1) {
            let candidate_rows = count.max(1).div_ceil(candidate_columns);
            let slot_width = ((content_width - gap * candidate_columns.saturating_sub(1) as f32)
                / candidate_columns as f32)
                .max(1.0);
            let slot_height = ((content_height - gap * candidate_rows.saturating_sub(1) as f32)
                / candidate_rows as f32)
                .max(1.0);
            let candidate_width = slot_width.min(slot_height * CARD_ASPECT_RATIO);
            let candidate_height = candidate_width / CARD_ASPECT_RATIO;
            let area = candidate_width * candidate_height;
            let empty = candidate_columns * candidate_rows - count;
            if area > best_area || area == best_area && empty < best_empty {
                best_area = area;
                best_empty = empty;
                columns = candidate_columns;
                rows = candidate_rows;
                card_width = candidate_width;
                card_height = candidate_height;
            }
        }
        let grid_height = rows as f32 * card_height + rows.saturating_sub(1) as f32 * gap;
        let start_y = top_padding + (content_height - grid_height) * 0.5;
        let cards = (0..count)
            .map(|index| {
                let row = index / columns;
                let column = index % columns;
                let row_start = row * columns;
                let row_count = (count - row_start).min(columns);
                let row_width =
                    row_count as f32 * card_width + row_count.saturating_sub(1) as f32 * gap;
                let start_x = (viewport_width - row_width) * 0.5;
                Bounds::new(
                    point(
                        px(start_x + column as f32 * (card_width + gap)),
                        px(start_y + row as f32 * (card_height + gap)),
                    ),
                    size(px(card_width), px(card_height)),
                )
            })
            .collect();
        let focus = centered_fit_bounds(viewport, px(18.0), CARD_ASPECT_RATIO);
        Self {
            columns,
            cards,
            focus,
        }
    }
}

fn centered_fit_bounds(
    viewport: Size<Pixels>,
    padding: Pixels,
    aspect_ratio: f32,
) -> Bounds<Pixels> {
    let width = (f32::from(viewport.width) - f32::from(padding) * 2.0).max(1.0);
    let height = (f32::from(viewport.height) - f32::from(padding) * 2.0).max(1.0);
    let fitted_width = width.min(height * aspect_ratio);
    let fitted_height = fitted_width / aspect_ratio;
    Bounds::new(
        point(
            px((f32::from(viewport.width) - fitted_width) * 0.5),
            px((f32::from(viewport.height) - fitted_height) * 0.5),
        ),
        size(px(fitted_width), px(fitted_height)),
    )
}

fn stacked_bounds(focus: Bounds<Pixels>, index: usize) -> Bounds<Pixels> {
    let width = focus.size.width * 0.58;
    let height = focus.size.height * 0.58;
    let offset = px((index % 7) as f32 * 4.0 - 12.0);
    Bounds::new(
        point(
            focus.origin.x + (focus.size.width - width) * 0.5 + offset,
            focus.origin.y + (focus.size.height - height) * 0.5 + offset,
        ),
        size(width, height),
    )
}

fn lerp_bounds(from: Bounds<Pixels>, to: Bounds<Pixels>, delta: f32) -> Bounds<Pixels> {
    Bounds::new(
        point(
            lerp_pixels(from.origin.x, to.origin.x, delta),
            lerp_pixels(from.origin.y, to.origin.y, delta),
        ),
        size(
            lerp_pixels(from.size.width, to.size.width, delta),
            lerp_pixels(from.size.height, to.size.height, delta),
        ),
    )
}

fn lerp_pixels(from: Pixels, to: Pixels, delta: f32) -> Pixels {
    from + (to - from) * delta
}

pub(crate) fn overview_available(mux: &MuxClient, route: WorkspaceRoute) -> bool {
    if route != WorkspaceRoute::App || !mux.is_connected() {
        return false;
    }
    let Some(attached) = mux.attached_session() else {
        return false;
    };
    mux.snapshot()
        .sessions
        .iter()
        .find(|session| session.id == attached)
        .is_some_and(|session| !session.windows.is_empty())
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use gpui::{KeyContext, Keymap, Keystroke};

    use super::*;

    #[test]
    fn grid_balances_cards_and_centers_incomplete_rows() {
        let grid = OverviewGrid::new(size(px(1_200.0), px(800.0)), 5);

        assert_eq!(grid.columns, 3);
        assert_eq!(grid.cards.len(), 5);
        assert!(grid.cards.iter().all(|bounds| {
            bounds.origin.x >= px(0.0)
                && bounds.origin.y >= px(0.0)
                && bounds.right() <= px(1_200.0)
                && bounds.bottom() <= px(800.0)
        }));
        assert!(grid.cards[3].origin.x > grid.cards[0].origin.x);
        assert!(grid.cards[4].origin.x < grid.cards[2].origin.x);
    }

    #[test]
    fn directional_selection_clamps_at_real_cards() {
        assert_eq!(moved_index(0, 5, 3, OverviewMove::Left), 0);
        assert_eq!(moved_index(1, 5, 3, OverviewMove::Down), 4);
        assert_eq!(moved_index(2, 5, 3, OverviewMove::Down), 4);
        assert_eq!(moved_index(4, 5, 3, OverviewMove::Right), 4);
        assert_eq!(moved_index(3, 5, 3, OverviewMove::Up), 0);
    }

    #[test]
    fn overview_shortcut_uses_the_root_context() {
        let binding = &key_bindings()[0];
        let keystroke = Keystroke::parse(if cfg!(any(target_os = "macos", target_os = "ios")) {
            "cmd-shift-o"
        } else {
            "ctrl-shift-o"
        })
        .expect("valid overview shortcut");
        let mut context = KeyContext::new_with_defaults();
        context.add(zz_ui::ROOT_KEY_CONTEXT);
        let keymap = Keymap::new(key_bindings().into());

        assert_eq!(
            binding.action().as_any().type_id(),
            TypeId::of::<ToggleWindowOverview>()
        );
        let (bindings, pending) = keymap.bindings_for_input(
            std::slice::from_ref(&keystroke),
            std::slice::from_ref(&context),
        );
        assert!(!pending);
        assert_eq!(bindings.len(), 1);
        assert_eq!(
            bindings[0].action().as_any().type_id(),
            binding.action().as_any().type_id()
        );
    }
}
