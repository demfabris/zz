use zz_protocol::{
    Axis, LayoutNode, PaneBorderIndicators, PaneBorderLines, PaneBorderStatus, PaneId,
    PopupBorderLines,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const fn contains(self, column: u16, row: u16) -> bool {
        column >= self.x
            && row >= self.y
            && column < self.x.saturating_add(self.width)
            && row < self.y.saturating_add(self.height)
    }

    pub const fn content(self) -> Self {
        Self {
            x: self.x,
            y: self.y.saturating_add(1),
            width: self.width,
            height: self.height.saturating_sub(1),
        }
    }

    pub fn inset(self, amount: u16) -> Self {
        Self {
            x: self.x.saturating_add(amount.min(self.width)),
            y: self.y.saturating_add(amount.min(self.height)),
            width: self.width.saturating_sub(amount.saturating_mul(2)),
            height: self.height.saturating_sub(amount.saturating_mul(2)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FloatingSpec {
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    pub client_columns: u16,
    pub client_rows: u16,
    pub border_lines: PopupBorderLines,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FloatingLayout {
    pub frame: Rect,
    pub content: Rect,
}

pub(crate) fn resolve_floating(spec: FloatingSpec, bounds: Rect) -> Option<FloatingLayout> {
    let width = spec.width.min(bounds.width);
    let height = spec.height.min(bounds.height);
    if width == 0 || height == 0 {
        return None;
    }
    let grid_left = bounds
        .x
        .saturating_add(bounds.width.saturating_sub(spec.client_columns) / 2);
    let grid_top = bounds
        .y
        .saturating_add(bounds.height.saturating_sub(spec.client_rows) / 2);
    let frame = Rect {
        x: grid_left
            .saturating_add(spec.left)
            .min(bounds.x.saturating_add(bounds.width.saturating_sub(width))),
        y: grid_top.saturating_add(spec.top).min(
            bounds
                .y
                .saturating_add(bounds.height.saturating_sub(height)),
        ),
        width,
        height,
    };
    let content = if spec.border_lines == PopupBorderLines::None {
        frame
    } else {
        frame.inset(1)
    };
    Some(FloatingLayout { frame, content })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PaneRect {
    pub pane: PaneId,
    pub rect: Rect,
    /// `pane-border-status bottom` puts the pane's status row on its last row
    /// instead of its first, the way `layout_fix_panes` leaves `yoff` alone
    /// and only shrinks `sy`.
    pub status_at_bottom: bool,
}

impl PaneRect {
    pub const fn content(self) -> Rect {
        if self.status_at_bottom {
            Rect {
                height: self.rect.height.saturating_sub(1),
                ..self.rect
            }
        } else {
            self.rect.content()
        }
    }

    pub fn status_row(self) -> Rect {
        let y = if self.status_at_bottom {
            self.rect
                .y
                .saturating_add(self.rect.height.saturating_sub(1))
        } else {
            self.rect.y
        };
        Rect {
            x: self.rect.x,
            y,
            width: self.rect.width,
            height: u16::from(self.rect.height > 0),
        }
    }
}

/// The border glyph tables `window_get_border_cell` picks from, indexed by the
/// `CELL_*` cell type `redraw_get_cell_type` computes: inside, UD, LR, RD, LD,
/// RU, LU, LRD, LRU, URD, ULD, LRUD, none.
const SINGLE_BORDERS: [&str; 13] = [
    " ", "\u{2502}", "\u{2500}", "\u{250c}", "\u{2510}", "\u{2514}", "\u{2518}", "\u{252c}",
    "\u{2534}", "\u{251c}", "\u{2524}", "\u{253c}", "\u{00b7}",
];
const DOUBLE_BORDERS: [&str; 13] = [
    " ", "\u{2551}", "\u{2550}", "\u{2554}", "\u{2557}", "\u{255a}", "\u{255d}", "\u{2566}",
    "\u{2569}", "\u{2560}", "\u{2563}", "\u{256c}", "\u{00b7}",
];
const HEAVY_BORDERS: [&str; 13] = [
    " ", "\u{2503}", "\u{2501}", "\u{250f}", "\u{2513}", "\u{2517}", "\u{251b}", "\u{2533}",
    "\u{253b}", "\u{2523}", "\u{252b}", "\u{254b}", "\u{00b7}",
];
const SIMPLE_BORDERS: [&str; 13] = [
    " ", "|", "-", "+", "+", "+", "+", "+", "+", "+", "+", "+", ".",
];
const BLANK_BORDERS: [&str; 13] = [
    " ", " ", " ", " ", " ", " ", " ", " ", " ", " ", " ", " ", " ",
];

pub(crate) const CELL_UD: usize = 1;
pub(crate) const CELL_LR: usize = 2;
pub(crate) const CELL_NONE: usize = 12;

/// `window_get_border_cell`: the glyph for one border cell. `number` draws the
/// owning pane's index instead of a line, except on `CELL_NONE`, which keeps
/// the single-line bullet.
pub(crate) fn border_glyph(lines: PaneBorderLines, cell_type: usize, index: Option<u32>) -> String {
    let cell_type = cell_type.min(CELL_NONE);
    match lines {
        PaneBorderLines::Single => SINGLE_BORDERS[cell_type].to_owned(),
        PaneBorderLines::Double => DOUBLE_BORDERS[cell_type].to_owned(),
        PaneBorderLines::Heavy => HEAVY_BORDERS[cell_type].to_owned(),
        PaneBorderLines::Simple => SIMPLE_BORDERS[cell_type].to_owned(),
        PaneBorderLines::Spaces | PaneBorderLines::None => BLANK_BORDERS[cell_type].to_owned(),
        PaneBorderLines::Number => {
            if cell_type == CELL_NONE {
                SINGLE_BORDERS[CELL_NONE].to_owned()
            } else {
                index.map_or_else(|| "*".to_owned(), |index| (index % 10).to_string())
            }
        }
    }
}

pub(crate) const BORDER_L: u8 = 1;
pub(crate) const BORDER_R: u8 = 2;
pub(crate) const BORDER_U: u8 = 4;
pub(crate) const BORDER_D: u8 = 8;

/// `redraw_get_cell_type`: the cell type a border cell takes from the mask of
/// the directions in which it continues.
pub(crate) const fn cell_type_of(mask: u8) -> usize {
    match mask {
        m if m == BORDER_L | BORDER_R | BORDER_U | BORDER_D => 11,
        m if m == BORDER_L | BORDER_R | BORDER_U => 8,
        m if m == BORDER_L | BORDER_R | BORDER_D => 7,
        m if m == BORDER_L | BORDER_U | BORDER_D => 10,
        m if m == BORDER_L | BORDER_U => 6,
        m if m == BORDER_L | BORDER_D => 4,
        m if m == BORDER_R | BORDER_U | BORDER_D => 9,
        m if m == BORDER_R | BORDER_U => 5,
        m if m == BORDER_R | BORDER_D => 3,
        m if m == BORDER_L | BORDER_R || m == BORDER_L || m == BORDER_R => CELL_LR,
        m if m == BORDER_U | BORDER_D || m == BORDER_U || m == BORDER_D => CELL_UD,
        _ => CELL_NONE,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Divider {
    pub rect: Rect,
    pub axis: Axis,
    pub highlighted: bool,
    pub style_pane: Option<PaneId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BorderSide {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BorderOwners {
    top: Option<(usize, PaneId)>,
    bottom: Option<(usize, PaneId)>,
    left: Option<(usize, PaneId)>,
    right: Option<(usize, PaneId)>,
}

impl BorderOwners {
    /// `redraw_build_scene` walks `w->z_index` in reverse and each pane
    /// overwrites the side it owns, so the pane earliest in that order wins a
    /// tie. `rank` is the pane's position in it.
    fn mark(&mut self, side: BorderSide, pane: PaneId, rank: usize) {
        let owner = match side {
            BorderSide::Top => &mut self.top,
            BorderSide::Bottom => &mut self.bottom,
            BorderSide::Left => &mut self.left,
            BorderSide::Right => &mut self.right,
        };
        if owner.is_none_or(|current| (rank, pane) < current) {
            *owner = Some((rank, pane));
        }
    }

    fn contains(self, pane: PaneId) -> bool {
        [self.top, self.bottom, self.left, self.right]
            .into_iter()
            .flatten()
            .any(|(_, owner)| owner == pane)
    }

    fn first(self) -> Option<PaneId> {
        self.top
            .or(self.bottom)
            .or(self.left)
            .or(self.right)
            .map(|(_, pane)| pane)
    }

    /// `redraw_mark_two_pane_colours`: with exactly two tiled panes and
    /// `pane-border-indicators` on `colour` or `both`, the divider carries each
    /// pane's own style over the half of it that pane sits on, and that choice
    /// outranks the active pane the way `style_wp` outranks `active` in
    /// `redraw_get_pane_for_border_style`.
    fn two_pane_colour(self, axis: Axis, bounds: Rect, position: u16) -> Option<PaneId> {
        let (near, far, midpoint) = match axis {
            Axis::Horizontal => (
                self.left?,
                self.right?,
                bounds.y.saturating_add(bounds.height / 2),
            ),
            Axis::Vertical => (
                self.top?,
                self.bottom?,
                bounds.x.saturating_add(bounds.width / 2),
            ),
        };
        Some(if position <= midpoint { near.1 } else { far.1 })
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResolvedLayout {
    pub panes: Vec<PaneRect>,
    pub dividers: Vec<Divider>,
}

pub(crate) fn resolve(
    node: &LayoutNode,
    rect: Rect,
    active_pane: PaneId,
    status: PaneBorderStatus,
    z_order: &[PaneId],
    indicators: PaneBorderIndicators,
) -> ResolvedLayout {
    let mut resolved = ResolvedLayout::default();
    collect(
        node,
        rect,
        active_pane,
        status == PaneBorderStatus::Bottom,
        &mut resolved,
    );
    let split_colours = indicators.colours() && resolved.panes.len() == 2;
    resolved.dividers = resolved
        .dividers
        .iter()
        .flat_map(|divider| {
            partition_divider(
                *divider,
                &resolved.panes,
                active_pane,
                z_order,
                split_colours.then_some(rect),
            )
        })
        .collect();
    resolved
}

fn partition_divider(
    divider: Divider,
    panes: &[PaneRect],
    active: PaneId,
    z_order: &[PaneId],
    split_colours: Option<Rect>,
) -> Vec<Divider> {
    let (length, thickness) = match divider.axis {
        Axis::Horizontal => (divider.rect.height, divider.rect.width),
        Axis::Vertical => (divider.rect.width, divider.rect.height),
    };
    if length == 0 || thickness == 0 {
        return vec![divider];
    }

    let mut spans: Vec<Divider> = Vec::new();
    for offset in 0..length {
        let position = match divider.axis {
            Axis::Horizontal => divider.rect.y.saturating_add(offset),
            Axis::Vertical => divider.rect.x.saturating_add(offset),
        };
        let owners = border_owners(panes, divider, position, z_order);
        let style_pane = split_colours
            .and_then(|bounds| owners.two_pane_colour(divider.axis, bounds, position))
            .or_else(|| owners.contains(active).then_some(active))
            .or_else(|| owners.first())
            .or(divider.style_pane);
        let highlighted = style_pane == Some(active);
        let cell = Divider {
            rect: match divider.axis {
                Axis::Horizontal => Rect {
                    y: position,
                    height: 1,
                    ..divider.rect
                },
                Axis::Vertical => Rect {
                    x: position,
                    width: 1,
                    ..divider.rect
                },
            },
            axis: divider.axis,
            highlighted,
            style_pane,
        };

        if let Some(span) = spans.last_mut()
            && span.highlighted == cell.highlighted
            && span.style_pane == cell.style_pane
        {
            match divider.axis {
                Axis::Horizontal => {
                    span.rect.height = span.rect.height.saturating_add(1);
                }
                Axis::Vertical => {
                    span.rect.width = span.rect.width.saturating_add(1);
                }
            }
            continue;
        }
        spans.push(cell);
    }
    spans
}

fn border_owners(
    panes: &[PaneRect],
    divider: Divider,
    position: u16,
    z_order: &[PaneId],
) -> BorderOwners {
    let (column, row) = match divider.axis {
        Axis::Horizontal => (divider.rect.x, position),
        Axis::Vertical => (position, divider.rect.y),
    };
    let mut owners = BorderOwners::default();
    for pane in panes {
        let rank = z_order
            .iter()
            .position(|candidate| *candidate == pane.pane)
            .unwrap_or(usize::MAX);
        mark_pane_owners(&mut owners, *pane, column, row, rank);
    }
    owners
}

fn mark_pane_owners(owners: &mut BorderOwners, pane: PaneRect, column: u16, row: u16, rank: usize) {
    let left = pane.rect.x.saturating_sub(1);
    let right = pane.rect.x.saturating_add(pane.rect.width);
    let top = pane.rect.y.saturating_sub(1);
    let bottom = pane.rect.y.saturating_add(pane.rect.height);
    let within_columns = column >= left && column <= right;
    let within_rows = row >= top && row <= bottom;

    if row == bottom && within_columns {
        owners.mark(BorderSide::Top, pane.pane, rank);
    }
    if pane.rect.y > 0 && row == top && within_columns {
        owners.mark(BorderSide::Bottom, pane.pane, rank);
    }
    if column == right && within_rows {
        owners.mark(BorderSide::Left, pane.pane, rank);
    }
    if pane.rect.x > 0 && column == left && within_rows {
        owners.mark(BorderSide::Right, pane.pane, rank);
    }
}

fn collect(
    node: &LayoutNode,
    rect: Rect,
    active: PaneId,
    status_at_bottom: bool,
    output: &mut ResolvedLayout,
) {
    match node {
        LayoutNode::Pane(pane) => output.panes.push(PaneRect {
            pane: *pane,
            rect,
            status_at_bottom,
        }),
        LayoutNode::Split {
            axis,
            ratio,
            first,
            second,
            ..
        } => {
            let ratio = resolved_ratio(*ratio);
            let extent = match axis {
                Axis::Horizontal => rect.width,
                Axis::Vertical => rect.height,
            };
            let divider_extent = u16::from(extent > 0);
            let available = extent.saturating_sub(divider_extent);
            let first_extent = scaled_extent(available, ratio);
            let second_extent = available.saturating_sub(first_extent);
            let (first_rect, divider_rect, second_rect) = match axis {
                Axis::Horizontal => (
                    Rect {
                        width: first_extent,
                        ..rect
                    },
                    Rect {
                        x: rect.x.saturating_add(first_extent),
                        y: rect.y,
                        width: divider_extent,
                        height: rect.height,
                    },
                    Rect {
                        x: rect
                            .x
                            .saturating_add(first_extent)
                            .saturating_add(divider_extent),
                        width: second_extent,
                        ..rect
                    },
                ),
                Axis::Vertical => (
                    Rect {
                        height: first_extent,
                        ..rect
                    },
                    Rect {
                        x: rect.x,
                        y: rect.y.saturating_add(first_extent),
                        width: rect.width,
                        height: divider_extent,
                    },
                    Rect {
                        y: rect
                            .y
                            .saturating_add(first_extent)
                            .saturating_add(divider_extent),
                        height: second_extent,
                        ..rect
                    },
                ),
            };
            let highlighted = first.contains(active) || second.contains(active);
            output.dividers.push(Divider {
                rect: divider_rect,
                axis: *axis,
                highlighted,
                style_pane: if highlighted {
                    Some(active)
                } else {
                    first_pane(first).or_else(|| first_pane(second))
                },
            });
            collect(first, first_rect, active, status_at_bottom, output);
            collect(second, second_rect, active, status_at_bottom, output);
        }
    }
}

fn first_pane(node: &LayoutNode) -> Option<PaneId> {
    match node {
        LayoutNode::Pane(pane) => Some(*pane),
        LayoutNode::Split { first, second, .. } => first_pane(first).or_else(|| first_pane(second)),
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the ratio is clamped and the product cannot exceed the u16 input extent"
)]
fn scaled_extent(available: u16, ratio: f32) -> u16 {
    (f32::from(available) * ratio).floor() as u16
}

fn resolved_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `layout_fix_panes` leaves `yoff` alone under `pane-border-status bottom`
    /// and only shrinks `sy`, so the content box starts on the pane's first row
    /// there and one row down under `top`; the mouse route has to read the
    /// same box the renderer paints.
    #[test]
    fn pane_content_starts_on_the_first_row_when_the_status_is_at_the_bottom() {
        let rect = Rect {
            x: 3,
            y: 5,
            width: 20,
            height: 8,
        };
        let top = PaneRect {
            pane: PaneId(0),
            rect,
            status_at_bottom: false,
        };
        let bottom = PaneRect {
            pane: PaneId(0),
            rect,
            status_at_bottom: true,
        };
        assert_eq!(top.content(), rect.content());
        assert_eq!(top.content().y, 6);
        assert_eq!(bottom.content().y, 5);
        assert_eq!(bottom.content().height, 7);
        assert!(bottom.content().contains(3, 5));
        assert!(!bottom.content().contains(3, 12));
        assert!(!top.content().contains(3, 5));
        assert_eq!(bottom.status_row().y, 12);
    }
    use zz_protocol::SplitId;

    fn pane(id: u64) -> LayoutNode {
        LayoutNode::Pane(PaneId(id))
    }

    fn split(axis: Axis, ratio: f32) -> LayoutNode {
        split_nodes(1, axis, ratio, pane(1), pane(2))
    }

    fn split_nodes(
        id: u64,
        axis: Axis,
        ratio: f32,
        first: LayoutNode,
        second: LayoutNode,
    ) -> LayoutNode {
        LayoutNode::Split {
            id: SplitId(id),
            axis,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    #[test]
    fn horizontal_split_floors_first_and_gives_remainder_to_second() {
        let resolved = resolve(
            &split(Axis::Horizontal, 0.5),
            Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 4,
            },
            PaneId(2),
            PaneBorderStatus::Off,
            &[PaneId(1), PaneId(2)],
            PaneBorderIndicators::Off,
        );

        assert_eq!(resolved.panes[0].rect.width, 4);
        assert_eq!(resolved.dividers[0].rect.x, 4);
        assert_eq!(resolved.dividers[0].rect.width, 1);
        assert_eq!(resolved.panes[1].rect.x, 5);
        assert_eq!(resolved.panes[1].rect.width, 5);
        assert!(resolved.dividers[0].highlighted);
    }

    #[test]
    fn non_finite_vertical_ratio_uses_half_and_tiles_deterministically() {
        let resolved = resolve(
            &split(Axis::Vertical, f32::NAN),
            Rect {
                x: 3,
                y: 2,
                width: 8,
                height: 8,
            },
            PaneId(1),
            PaneBorderStatus::Off,
            &[PaneId(1), PaneId(2)],
            PaneBorderIndicators::Colour,
        );

        assert_eq!(resolved.panes[0].rect.height, 3);
        assert_eq!(resolved.dividers[0].rect.y, 5);
        assert_eq!(resolved.panes[1].rect.y, 6);
        assert_eq!(resolved.panes[1].rect.height, 4);
    }

    #[test]
    fn out_of_range_ratios_clamp_after_reserving_the_divider() {
        let positive = resolve(
            &split(Axis::Horizontal, f32::INFINITY),
            Rect {
                width: 6,
                height: 2,
                ..Rect::default()
            },
            PaneId(1),
            PaneBorderStatus::Off,
            &[PaneId(1), PaneId(2)],
            PaneBorderIndicators::Colour,
        );
        let negative = resolve(
            &split(Axis::Horizontal, -2.0),
            Rect {
                width: 6,
                height: 2,
                ..Rect::default()
            },
            PaneId(1),
            PaneBorderStatus::Off,
            &[PaneId(1), PaneId(2)],
            PaneBorderIndicators::Colour,
        );

        assert_eq!(
            (positive.panes[0].rect.width, positive.panes[1].rect.width),
            (2, 3)
        );
        assert_eq!(
            (negative.panes[0].rect.width, negative.panes[1].rect.width),
            (0, 5)
        );
        assert_eq!(positive.dividers[0].rect.width, 1);
        assert_eq!(negative.dividers[0].rect.width, 1);
    }

    #[test]
    fn collapsed_active_pane_keeps_its_visible_divider() {
        let resolved = resolve(
            &split(Axis::Horizontal, 0.5),
            Rect {
                width: 2,
                height: 2,
                ..Rect::default()
            },
            PaneId(1),
            PaneBorderStatus::Off,
            &[PaneId(1), PaneId(2)],
            PaneBorderIndicators::Colour,
        );

        assert_eq!(
            (resolved.panes[0].rect.width, resolved.panes[1].rect.width),
            (0, 1)
        );
        assert_eq!(
            (resolved.dividers[0].rect.x, resolved.dividers[0].rect.width),
            (0, 1)
        );
        assert!(resolved.dividers[0].highlighted);
        assert_eq!(resolved.dividers[0].style_pane, Some(PaneId(1)));
    }

    #[test]
    fn divider_style_pane_prefers_an_active_pane_in_either_subtree() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 4,
        };
        let adjacent = resolve(
            &split(Axis::Horizontal, 0.5),
            rect,
            PaneId(2),
            PaneBorderStatus::Off,
            &[],
            PaneBorderIndicators::Off,
        );
        assert!(adjacent.dividers[0].highlighted);
        assert_eq!(adjacent.dividers[0].style_pane, Some(PaneId(2)));

        let elsewhere = resolve(
            &split(Axis::Horizontal, 0.5),
            rect,
            PaneId(9),
            PaneBorderStatus::Off,
            &[],
            PaneBorderIndicators::Off,
        );
        assert!(!elsewhere.dividers[0].highlighted);
        assert_eq!(elsewhere.dividers[0].style_pane, Some(PaneId(1)));
    }

    #[test]
    fn two_pane_colour_indicators_split_the_divider_by_position() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 6,
        };
        let split_colours = resolve(
            &split(Axis::Horizontal, 0.5),
            rect,
            PaneId(2),
            PaneBorderStatus::Off,
            &[PaneId(1), PaneId(2)],
            PaneBorderIndicators::Colour,
        );
        let owners = split_colours
            .dividers
            .iter()
            .flat_map(|divider| {
                (divider.rect.y..divider.rect.y.saturating_add(divider.rect.height))
                    .map(move |row| (row, divider.style_pane))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            owners,
            vec![
                (0, Some(PaneId(1))),
                (1, Some(PaneId(1))),
                (2, Some(PaneId(1))),
                (3, Some(PaneId(1))),
                (4, Some(PaneId(2))),
                (5, Some(PaneId(2))),
            ]
        );

        let whole = resolve(
            &split(Axis::Horizontal, 0.5),
            rect,
            PaneId(2),
            PaneBorderStatus::Off,
            &[PaneId(1), PaneId(2)],
            PaneBorderIndicators::Arrows,
        );
        assert_eq!(whole.dividers.len(), 1);
        assert_eq!(whole.dividers[0].style_pane, Some(PaneId(2)));
    }

    #[test]
    fn perpendicular_split_partitions_outer_divider_and_gives_junction_to_active_pane() {
        let layout = split_nodes(
            1,
            Axis::Horizontal,
            0.5,
            pane(1),
            split_nodes(2, Axis::Vertical, 0.5, pane(2), pane(3)),
        );

        let resolved = resolve(
            &layout,
            Rect {
                width: 11,
                height: 9,
                ..Rect::default()
            },
            PaneId(3),
            PaneBorderStatus::Off,
            &[],
            PaneBorderIndicators::Colour,
        );

        assert_eq!(
            resolved.dividers,
            vec![
                Divider {
                    rect: Rect {
                        x: 5,
                        width: 1,
                        height: 4,
                        ..Rect::default()
                    },
                    axis: Axis::Horizontal,
                    highlighted: false,
                    style_pane: Some(PaneId(1)),
                },
                Divider {
                    rect: Rect {
                        x: 5,
                        y: 4,
                        width: 1,
                        height: 5,
                    },
                    axis: Axis::Horizontal,
                    highlighted: true,
                    style_pane: Some(PaneId(3)),
                },
                Divider {
                    rect: Rect {
                        x: 6,
                        y: 4,
                        width: 5,
                        height: 1,
                    },
                    axis: Axis::Vertical,
                    highlighted: true,
                    style_pane: Some(PaneId(3)),
                },
            ]
        );
    }

    #[test]
    fn same_axis_hidden_active_pane_does_not_own_ancestor_divider() {
        let layout = split_nodes(
            1,
            Axis::Horizontal,
            0.7,
            split_nodes(2, Axis::Horizontal, 0.5, pane(1), pane(2)),
            pane(3),
        );

        let resolved = resolve(
            &layout,
            Rect {
                width: 15,
                height: 5,
                ..Rect::default()
            },
            PaneId(1),
            PaneBorderStatus::Off,
            &[PaneId(1), PaneId(2)],
            PaneBorderIndicators::Colour,
        );

        assert_eq!(resolved.dividers[0].rect.x, 9);
        assert_eq!(resolved.dividers[0].rect.height, 5);
        assert!(!resolved.dividers[0].highlighted);
        assert_eq!(resolved.dividers[0].style_pane, Some(PaneId(2)));
    }

    #[test]
    fn inactive_junction_uses_directional_owner_priority() {
        let layout = split_nodes(
            1,
            Axis::Horizontal,
            0.5,
            pane(1),
            split_nodes(2, Axis::Vertical, 0.5, pane(2), pane(3)),
        );

        let resolved = resolve(
            &layout,
            Rect {
                width: 11,
                height: 9,
                ..Rect::default()
            },
            PaneId(9),
            PaneBorderStatus::Off,
            &[],
            PaneBorderIndicators::Colour,
        );

        assert_eq!(resolved.dividers[0].style_pane, Some(PaneId(1)));
        assert_eq!(resolved.dividers[0].rect.height, 4);
        assert_eq!(resolved.dividers[1].style_pane, Some(PaneId(2)));
        assert_eq!(resolved.dividers[1].rect.y, 4);
        assert_eq!(resolved.dividers[1].rect.height, 1);
        assert_eq!(resolved.dividers[2].style_pane, Some(PaneId(1)));
        assert_eq!(resolved.dividers[2].rect.y, 5);
        assert_eq!(resolved.dividers[2].rect.height, 4);
        assert!(resolved.dividers.iter().all(|divider| !divider.highlighted));
    }

    #[test]
    fn split_only_cross_uses_creation_order_for_direction_ties() {
        let layout = split_nodes(
            1,
            Axis::Vertical,
            0.5,
            split_nodes(2, Axis::Horizontal, 0.5, pane(3), pane(1)),
            split_nodes(3, Axis::Horizontal, 0.5, pane(4), pane(2)),
        );

        let resolved = resolve(
            &layout,
            Rect {
                width: 11,
                height: 9,
                ..Rect::default()
            },
            PaneId(9),
            PaneBorderStatus::Off,
            &[],
            PaneBorderIndicators::Colour,
        );

        assert_eq!(resolved.dividers[0].rect.width, 5);
        assert_eq!(resolved.dividers[0].style_pane, Some(PaneId(3)));
        assert_eq!(resolved.dividers[1].rect.x, 5);
        assert_eq!(resolved.dividers[1].rect.width, 6);
        assert_eq!(resolved.dividers[1].style_pane, Some(PaneId(1)));
        assert!(resolved.dividers.iter().all(|divider| !divider.highlighted));
    }

    #[test]
    fn zero_extent_does_not_move_children_outside_the_parent() {
        let resolved = resolve(
            &split(Axis::Horizontal, 0.5),
            Rect {
                x: 7,
                width: 0,
                height: 2,
                ..Rect::default()
            },
            PaneId(1),
            PaneBorderStatus::Off,
            &[PaneId(1), PaneId(2)],
            PaneBorderIndicators::Colour,
        );

        assert_eq!(resolved.dividers[0].rect.width, 0);
        assert_eq!(resolved.panes[0].rect.x, 7);
        assert_eq!(resolved.panes[1].rect.x, 7);
    }

    #[test]
    fn floating_geometry_centres_the_published_grid_and_clamps_to_bounds() {
        let spec = FloatingSpec {
            left: 2,
            top: 1,
            width: 10,
            height: 4,
            client_columns: 20,
            client_rows: 10,
            border_lines: PopupBorderLines::Single,
        };
        assert_eq!(
            resolve_floating(
                spec,
                Rect {
                    x: 0,
                    y: 0,
                    width: 30,
                    height: 16,
                }
            ),
            Some(FloatingLayout {
                frame: Rect {
                    x: 7,
                    y: 4,
                    width: 10,
                    height: 4,
                },
                content: Rect {
                    x: 8,
                    y: 5,
                    width: 8,
                    height: 2,
                },
            })
        );
        assert_eq!(
            resolve_floating(
                spec,
                Rect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 3,
                }
            )
            .map(|layout| layout.frame),
            Some(Rect {
                x: 0,
                y: 0,
                width: 8,
                height: 3,
            })
        );
    }

    #[test]
    fn borderless_floating_content_uses_the_full_frame() {
        let bounds = Rect {
            width: 8,
            height: 4,
            ..Rect::default()
        };
        let spec = FloatingSpec {
            left: 0,
            top: 0,
            width: 8,
            height: 4,
            client_columns: 8,
            client_rows: 4,
            border_lines: PopupBorderLines::None,
        };
        let borderless = resolve_floating(spec, bounds).expect("borderless layout");
        assert_eq!(borderless.content, borderless.frame);

        let bordered = resolve_floating(
            FloatingSpec {
                border_lines: PopupBorderLines::Padded,
                ..spec
            },
            bounds,
        )
        .expect("padded layout");
        assert_eq!(
            bordered.content,
            Rect {
                x: 1,
                y: 1,
                width: 6,
                height: 2,
            }
        );
    }
}
