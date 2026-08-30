use zz_protocol::{Axis, LayoutNode, PaneId, PopupBorderLines};

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
    top: Option<PaneId>,
    bottom: Option<PaneId>,
    left: Option<PaneId>,
    right: Option<PaneId>,
}

impl BorderOwners {
    fn mark(&mut self, side: BorderSide, pane: PaneId) {
        let owner = match side {
            BorderSide::Top => &mut self.top,
            BorderSide::Bottom => &mut self.bottom,
            BorderSide::Left => &mut self.left,
            BorderSide::Right => &mut self.right,
        };
        *owner = Some(owner.map_or(pane, |current| current.min(pane)));
    }

    fn contains(self, pane: PaneId) -> bool {
        self.top == Some(pane)
            || self.bottom == Some(pane)
            || self.left == Some(pane)
            || self.right == Some(pane)
    }

    fn first(self) -> Option<PaneId> {
        self.top.or(self.bottom).or(self.left).or(self.right)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResolvedLayout {
    pub panes: Vec<PaneRect>,
    pub dividers: Vec<Divider>,
}

pub(crate) fn resolve(node: &LayoutNode, rect: Rect, active_pane: PaneId) -> ResolvedLayout {
    let mut resolved = ResolvedLayout::default();
    collect(node, rect, active_pane, &mut resolved);
    resolved.dividers = resolved
        .dividers
        .iter()
        .flat_map(|divider| partition_divider(*divider, &resolved.panes, active_pane))
        .collect();
    resolved
}

fn partition_divider(divider: Divider, panes: &[PaneRect], active: PaneId) -> Vec<Divider> {
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
        let owners = border_owners(panes, divider, position);
        let highlighted = owners.contains(active);
        let style_pane = highlighted
            .then_some(active)
            .or_else(|| owners.first())
            .or(divider.style_pane);
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

fn border_owners(panes: &[PaneRect], divider: Divider, position: u16) -> BorderOwners {
    let (column, row) = match divider.axis {
        Axis::Horizontal => (divider.rect.x, position),
        Axis::Vertical => (position, divider.rect.y),
    };
    let mut owners = BorderOwners::default();
    for pane in panes {
        mark_pane_owners(&mut owners, *pane, column, row);
    }
    owners
}

fn mark_pane_owners(owners: &mut BorderOwners, pane: PaneRect, column: u16, row: u16) {
    if pane.rect.width == 0 || pane.rect.height == 0 {
        return;
    }
    let left = pane.rect.x.saturating_sub(1);
    let right = pane.rect.x.saturating_add(pane.rect.width);
    let top = pane.rect.y.saturating_sub(1);
    let bottom = pane.rect.y.saturating_add(pane.rect.height);
    let within_columns = column >= left && column <= right;
    let within_rows = row >= top && row <= bottom;

    if row == bottom && within_columns {
        owners.mark(BorderSide::Top, pane.pane);
    }
    if pane.rect.y > 0 && row == top && within_columns {
        owners.mark(BorderSide::Bottom, pane.pane);
    }
    if column == right && within_rows {
        owners.mark(BorderSide::Left, pane.pane);
    }
    if pane.rect.x > 0 && column == left && within_rows {
        owners.mark(BorderSide::Right, pane.pane);
    }
}

fn collect(node: &LayoutNode, rect: Rect, active: PaneId, output: &mut ResolvedLayout) {
    match node {
        LayoutNode::Pane(pane) => output.panes.push(PaneRect { pane: *pane, rect }),
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
            collect(first, first_rect, active, output);
            collect(second, second_rect, active, output);
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
        );
        let negative = resolve(
            &split(Axis::Horizontal, -2.0),
            Rect {
                width: 6,
                height: 2,
                ..Rect::default()
            },
            PaneId(1),
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
    fn divider_style_pane_prefers_an_active_pane_in_either_subtree() {
        let rect = Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 4,
        };
        let adjacent = resolve(&split(Axis::Horizontal, 0.5), rect, PaneId(2));
        assert!(adjacent.dividers[0].highlighted);
        assert_eq!(adjacent.dividers[0].style_pane, Some(PaneId(2)));

        let elsewhere = resolve(&split(Axis::Horizontal, 0.5), rect, PaneId(9));
        assert!(!elsewhere.dividers[0].highlighted);
        assert_eq!(elsewhere.dividers[0].style_pane, Some(PaneId(1)));
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
