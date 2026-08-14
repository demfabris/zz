use std::time::Duration;

use gpui::{App, Bounds, KeyBinding, MIN_WINDOW_ZOOM, Pixels, Size, Window, point, px, size};
use zz_protocol::PaneId;
use zz_ui::{TITLE_BAR_HEIGHT, UiZoom, draws_window_controls};

use crate::mux::client::MuxClient;

use super::sidebar::{ChromeMode, WorkspaceRoute};

const OVERVIEW_PADDING: f32 = 32.0;
const CONTENT_ZOOM_BOOST: f32 = 1.2;

pub(crate) const OVERVIEW_OPEN_DURATION: Duration = Duration::from_millis(360);
pub(crate) const OVERVIEW_CLOSE_DURATION: Duration = Duration::from_millis(240);
pub(crate) const OVERVIEW_NORMAL_KEY_CONTEXT: &str = "WindowOverviewNormal";
pub(crate) const OVERVIEW_INSERT_KEY_CONTEXT: &str = "WindowOverviewInsert";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverviewDirection {
    Left,
    Right,
    Up,
    Down,
}

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

#[derive(Clone, Debug)]
pub(crate) struct OverviewGrid {
    pub(crate) columns: usize,
    pub(crate) groups: Vec<Bounds<Pixels>>,
}

impl OverviewGrid {
    pub(crate) fn new(viewport: Size<Pixels>, count: usize) -> Self {
        let columns = best_columns(viewport, count);
        Self::with_columns(viewport, count, columns)
    }

    pub(crate) fn with_columns(viewport: Size<Pixels>, count: usize, columns: usize) -> Self {
        Self::with_columns_scaled(viewport, count, columns, 1.0, Pixels::ZERO)
    }

    pub(crate) fn with_columns_scaled(
        viewport: Size<Pixels>,
        count: usize,
        columns: usize,
        metric_scale: f32,
        group_gap: Pixels,
    ) -> Self {
        Self::with_columns_scaled_and_top_inset(
            viewport,
            count,
            columns,
            metric_scale,
            group_gap,
            Pixels::ZERO,
        )
    }

    pub(crate) fn with_columns_scaled_and_top_inset(
        viewport: Size<Pixels>,
        count: usize,
        columns: usize,
        metric_scale: f32,
        group_gap: Pixels,
        top_inset: Pixels,
    ) -> Self {
        let requested_count = count;
        let count = count.max(1);
        let columns = columns.clamp(1, count);
        let rows = count.div_ceil(columns);
        let viewport_width = f32::from(viewport.width).max(1.0);
        let viewport_height = f32::from(viewport.height).max(1.0);
        let metric_scale = metric_scale.max(0.1);
        let side_padding = (OVERVIEW_PADDING * metric_scale).min(viewport_width * 0.08);
        let bottom_padding = (OVERVIEW_PADDING * metric_scale).min(viewport_height * 0.14);
        let top_padding = ((OVERVIEW_PADDING * metric_scale).min(viewport_height * 0.14)
            + f32::from(top_inset).max(0.0))
        .min((viewport_height - bottom_padding - 1.0).max(0.0));
        let content_width = (viewport_width - side_padding * 2.0).max(1.0);
        let content_height = (viewport_height - top_padding - bottom_padding).max(1.0);
        let gap = f32::from(group_gap)
            .max(0.0)
            .min((content_width / count as f32).max(4.0));
        let aspect_ratio = (viewport_width / viewport_height).max(0.1);
        let slot_width =
            ((content_width - gap * columns.saturating_sub(1) as f32) / columns as f32).max(1.0);
        let slot_height =
            ((content_height - gap * rows.saturating_sub(1) as f32) / rows as f32).max(1.0);
        let card_width = slot_width.min(slot_height * aspect_ratio);
        let card_height = card_width / aspect_ratio;
        let grid_height = rows as f32 * card_height + rows.saturating_sub(1) as f32 * gap;
        let start_y = top_padding + (content_height - grid_height) * 0.5;
        let groups = (0..count)
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
            .take(requested_count)
            .collect();
        Self { columns, groups }
    }
}

pub(crate) fn overview_titlebar_height(mode: ChromeMode, window: &Window, cx: &App) -> Pixels {
    if overview_titlebar_visible_for(
        mode,
        draws_window_controls(window),
        cfg!(target_os = "macos"),
        window.is_fullscreen(),
    ) {
        TITLE_BAR_HEIGHT * overview_metric_scale(UiZoom::get(cx), window.zoom())
    } else {
        Pixels::ZERO
    }
}

pub(crate) fn overview_metric_scale(ui_zoom: f32, window_zoom: f32) -> f32 {
    ui_zoom / window_zoom.max(0.1)
}

fn overview_titlebar_visible_for(
    mode: ChromeMode,
    draws_controls: bool,
    macos: bool,
    fullscreen: bool,
) -> bool {
    matches!(mode, ChromeMode::Titlebar) || draws_controls || macos && !fullscreen
}

fn best_columns(viewport: Size<Pixels>, count: usize) -> usize {
    let count = count.max(1);
    let viewport_width = f32::from(viewport.width).max(1.0);
    let viewport_height = f32::from(viewport.height).max(1.0);
    let aspect_ratio = (viewport_width / viewport_height).max(0.1);
    let mut best = 1;
    let mut best_area = 0.0;
    let mut best_empty = usize::MAX;
    for columns in 1..=count {
        let rows = count.div_ceil(columns);
        let width = viewport_width / columns as f32;
        let height = viewport_height / rows as f32;
        let card_width = width.min(height * aspect_ratio);
        let area = card_width * card_width / aspect_ratio;
        let empty = columns * rows - count;
        if area > best_area
            || area == best_area && (empty < best_empty || empty == best_empty && columns > best)
        {
            best = columns;
            best_area = area;
            best_empty = empty;
        }
    }
    best
}

pub(crate) fn overview_zoom(base_zoom: f32, physical_viewport: Size<Pixels>, count: usize) -> f32 {
    let grid = OverviewGrid::new(physical_viewport, count);
    let rows = count.max(1).div_ceil(grid.columns);
    (base_zoom * CONTENT_ZOOM_BOOST / grid.columns.max(rows) as f32)
        .min(base_zoom)
        .max(MIN_WINDOW_ZOOM)
}

pub(crate) fn overview_edge_bounds(
    viewport: Size<Pixels>,
    target: Bounds<Pixels>,
    group_gap: Pixels,
) -> Bounds<Pixels> {
    let left = f32::from(target.origin.x);
    let top = f32::from(target.origin.y);
    let right = f32::from(viewport.width - target.right());
    let bottom = f32::from(viewport.height - target.bottom());
    let gap = group_gap.max(Pixels::ZERO);
    let edge = [left, right, top, bottom]
        .into_iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.total_cmp(b))
        .map_or(0, |(edge, _)| edge);
    let origin = match edge {
        0 => point(-target.size.width - gap, target.origin.y),
        1 => point(viewport.width + gap, target.origin.y),
        2 => point(target.origin.x, -target.size.height - gap),
        _ => point(target.origin.x, viewport.height + gap),
    };
    Bounds::new(origin, target.size)
}

pub(crate) fn interpolate_bounds(
    from: Bounds<Pixels>,
    to: Bounds<Pixels>,
    progress: f32,
) -> Bounds<Pixels> {
    let progress = progress.clamp(0.0, 1.0);
    Bounds::new(
        point(
            from.origin.x + (to.origin.x - from.origin.x) * progress,
            from.origin.y + (to.origin.y - from.origin.y) * progress,
        ),
        size(
            from.size.width + (to.size.width - from.size.width) * progress,
            from.size.height + (to.size.height - from.size.height) * progress,
        ),
    )
}

pub(crate) fn next_overview_pane(
    current: PaneId,
    panes: &[(PaneId, Bounds<Pixels>)],
    direction: OverviewDirection,
) -> Option<PaneId> {
    let current_bounds = panes
        .iter()
        .find_map(|(pane, bounds)| (*pane == current).then_some(*bounds))?;
    let center = current_bounds.center();
    panes
        .iter()
        .filter_map(|(pane, bounds)| {
            if *pane == current {
                return None;
            }
            let candidate = bounds.center();
            let (primary, perpendicular) = match direction {
                OverviewDirection::Left => (
                    f32::from(center.x - candidate.x),
                    interval_separation(
                        current_bounds.origin.y,
                        current_bounds.bottom(),
                        bounds.origin.y,
                        bounds.bottom(),
                    ),
                ),
                OverviewDirection::Right => (
                    f32::from(candidate.x - center.x),
                    interval_separation(
                        current_bounds.origin.y,
                        current_bounds.bottom(),
                        bounds.origin.y,
                        bounds.bottom(),
                    ),
                ),
                OverviewDirection::Up => (
                    f32::from(center.y - candidate.y),
                    interval_separation(
                        current_bounds.origin.x,
                        current_bounds.right(),
                        bounds.origin.x,
                        bounds.right(),
                    ),
                ),
                OverviewDirection::Down => (
                    f32::from(candidate.y - center.y),
                    interval_separation(
                        current_bounds.origin.x,
                        current_bounds.right(),
                        bounds.origin.x,
                        bounds.right(),
                    ),
                ),
            };
            let cross = match direction {
                OverviewDirection::Left | OverviewDirection::Right => {
                    f32::from((candidate.y - center.y).abs())
                }
                OverviewDirection::Up | OverviewDirection::Down => {
                    f32::from((candidate.x - center.x).abs())
                }
            };
            (primary > 0.0).then_some((*pane, perpendicular.0, perpendicular.1, primary, cross))
        })
        .min_by(|left, right| {
            left.1
                .cmp(&right.1)
                .then_with(|| left.2.total_cmp(&right.2))
                .then_with(|| left.3.total_cmp(&right.3))
                .then_with(|| left.4.total_cmp(&right.4))
                .then_with(|| left.0.cmp(&right.0))
        })
        .map(|(pane, _, _, _, _)| pane)
}

fn interval_separation(
    first_start: Pixels,
    first_end: Pixels,
    second_start: Pixels,
    second_end: Pixels,
) -> (bool, f32) {
    if first_end > second_start && second_end > first_start {
        (false, 0.0)
    } else if first_end <= second_start {
        (true, f32::from(second_start - first_end))
    } else {
        (true, f32::from(first_start - second_end))
    }
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
    fn grid_balances_groups_and_centers_incomplete_rows() {
        let grid = OverviewGrid::new(size(px(1_200.0), px(800.0)), 5);

        assert_eq!(grid.columns, 3);
        assert_eq!(grid.groups.len(), 5);
        assert!(grid.groups.iter().all(|bounds| {
            bounds.origin.x >= px(0.0)
                && bounds.origin.y >= px(0.0)
                && bounds.right() <= px(1_200.0)
                && bounds.bottom() <= px(800.0)
        }));
        assert!(grid.groups[3].origin.x > grid.groups[0].origin.x);
        assert!(grid.groups[4].origin.x < grid.groups[2].origin.x);
    }

    #[test]
    fn window_groups_use_the_requested_pane_gap() {
        let viewport = size(px(1_200.0), px(800.0));
        let flush = OverviewGrid::with_columns_scaled(viewport, 2, 2, 1.0, Pixels::ZERO);
        let spaced = OverviewGrid::with_columns_scaled(viewport, 2, 2, 1.0, px(9.0));

        assert_eq!(flush.groups[0].right(), flush.groups[1].origin.x);
        assert_eq!(
            spaced.groups[1].origin.x - spaced.groups[0].right(),
            px(9.0)
        );
    }

    #[test]
    fn grid_reserves_titlebar_and_panorama_padding() {
        let viewport = size(px(1_200.0), px(800.0));
        let grid = OverviewGrid::with_columns_scaled_and_top_inset(
            viewport,
            2,
            2,
            1.0,
            Pixels::ZERO,
            px(34.0),
        );

        assert!(grid.groups.iter().all(|group| {
            group.origin.x >= px(32.0)
                && group.origin.y >= px(66.0)
                && group.right() <= viewport.width - px(32.0)
                && group.bottom() <= viewport.height - px(32.0)
        }));
    }

    #[test]
    fn panorama_titlebar_policy_covers_each_platform_chrome() {
        assert!(overview_titlebar_visible_for(
            ChromeMode::Titlebar,
            false,
            false,
            false,
        ));
        assert!(overview_titlebar_visible_for(
            ChromeMode::Sidebar,
            true,
            false,
            false,
        ));
        assert!(overview_titlebar_visible_for(
            ChromeMode::Sidebar,
            false,
            true,
            false,
        ));
        assert!(!overview_titlebar_visible_for(
            ChromeMode::Sidebar,
            false,
            true,
            true,
        ));
        assert!(!overview_titlebar_visible_for(
            ChromeMode::Sidebar,
            false,
            false,
            false,
        ));

        let scale = overview_metric_scale(1.3, 0.65);
        assert!((scale - 2.0).abs() < f32::EPSILON);
        assert!(
            (f32::from(TITLE_BAR_HEIGHT * scale * 0.65) - f32::from(TITLE_BAR_HEIGHT * 1.3)).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn panorama_zoom_composes_with_ui_zoom_and_clamps() {
        let viewport = size(px(1_200.0), px(800.0));

        assert_eq!(overview_zoom(1.3, viewport, 1), 1.3);
        assert!((overview_zoom(1.3, viewport, 4) - 0.78).abs() < f32::EPSILON);
        assert!((overview_zoom(1.2, viewport, 9) - 0.48).abs() < f32::EPSILON);
        assert_eq!(overview_zoom(0.5, viewport, 9), MIN_WINDOW_ZOOM);
    }

    #[test]
    fn groups_enter_from_their_nearest_viewport_edge() {
        let viewport = size(px(1_200.0), px(800.0));
        let top_left = Bounds::new(point(px(30.0), px(70.0)), size(px(500.0), px(300.0)));
        let bottom_right = Bounds::new(point(px(670.0), px(430.0)), size(px(500.0), px(300.0)));

        let first = overview_edge_bounds(viewport, top_left, px(12.0));
        let last = overview_edge_bounds(viewport, bottom_right, px(12.0));

        assert!(first.right() < px(0.0));
        assert!(last.origin.x > viewport.width);
        assert_eq!(interpolate_bounds(first, top_left, 1.0), top_left);
        assert_eq!(interpolate_bounds(first, top_left, 0.0), first);
        assert_eq!(
            interpolate_bounds(first, top_left, 0.5),
            Bounds::new(
                point(
                    (first.origin.x + top_left.origin.x) * 0.5,
                    (first.origin.y + top_left.origin.y) * 0.5,
                ),
                size(
                    (first.size.width + top_left.size.width) * 0.5,
                    (first.size.height + top_left.size.height) * 0.5,
                ),
            )
        );
    }

    #[test]
    fn directional_navigation_prefers_aligned_panes_across_window_groups() {
        let panes = [
            (
                PaneId(1),
                Bounds::new(point(px(0.0), px(0.0)), size(px(300.0), px(300.0))),
            ),
            (
                PaneId(2),
                Bounds::new(point(px(300.0), px(0.0)), size(px(300.0), px(150.0))),
            ),
            (
                PaneId(3),
                Bounds::new(point(px(300.0), px(150.0)), size(px(300.0), px(150.0))),
            ),
            (
                PaneId(4),
                Bounds::new(point(px(650.0), px(20.0)), size(px(300.0), px(300.0))),
            ),
        ];

        assert_eq!(
            next_overview_pane(PaneId(1), &panes, OverviewDirection::Right),
            Some(PaneId(2))
        );
        assert_eq!(
            next_overview_pane(PaneId(2), &panes, OverviewDirection::Down),
            Some(PaneId(3))
        );
        assert_eq!(
            next_overview_pane(PaneId(3), &panes, OverviewDirection::Right),
            Some(PaneId(4))
        );
        assert_eq!(
            next_overview_pane(PaneId(1), &panes, OverviewDirection::Left),
            None
        );
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
