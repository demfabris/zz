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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ResolvedLayout {
    pub panes: Vec<PaneRect>,
    pub dividers: Vec<Divider>,
}

pub(crate) fn resolve(node: &LayoutNode, rect: Rect, active_pane: PaneId) -> ResolvedLayout {
    let mut resolved = ResolvedLayout::default();
    collect(node, rect, active_pane, &mut resolved);
    resolved
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
        LayoutNode::Split {
            id: SplitId(1),
            axis,
            ratio,
            first: Box::new(pane(1)),
            second: Box::new(pane(2)),
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
